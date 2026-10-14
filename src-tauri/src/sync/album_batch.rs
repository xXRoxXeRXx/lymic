//! Dedicated batching, assignment, duplicate resolution, and auditing for Immich target albums.

use std::collections::BTreeMap;
use std::sync::Arc;
use serde_json::json;
use sqlx::SqlitePool;

use crate::{
    app::events::log_to_ui_file_error,
    audit::{self, audit_event, SyncAuditContext},
    db,
    immich::{AlbumId, BulkCheckDuplicate, ImmichClient, RemoteAssetId},
    media::model::HashedUploadUnit,
    sync::processor::uploaded_asset_states,
};

#[derive(Debug, Clone)]
pub(crate) struct PendingAlbumUnit {
    pub album_id: AlbumId,
    pub remote_id: RemoteAssetId,
    pub unit: HashedUploadUnit,
    pub uploaded_count: usize,
}

/// Resolves the remote asset ID for a duplicate file and returns a `PendingAlbumUnit`.
///
/// If `update_sync_queue_remote_id` fails to persist the ID to SQLite,
/// this returns `Err(...)` to abort the processing step.
pub(crate) async fn resolve_duplicate_for_album(
    client: &ImmichClient,
    pool: &SqlitePool,
    unit: &HashedUploadUnit,
    existing: &[BulkCheckDuplicate],
    album_id: &AlbumId,
) -> Result<PendingAlbumUnit, String> {
    let image_hash = &unit
        .image
        .checksums
        .as_ref()
        .expect("hashed unit has checksums")
        .sha1_base64;

    let resolved = match existing
        .iter()
        .find(|d| &d.hash == image_hash)
        .and_then(|d| d.asset_id.clone())
    {
        Some(id) => Ok(Some(id)),
        None => client.search_asset_by_checksum(image_hash).await,
    };

    match resolved {
        Ok(Some(remote_id)) => {
            db::update_sync_queue_remote_id(pool, &unit.image.path, &remote_id)
                .await
                .map_err(|err| {
                    format!(
                        "Failed to persist remote ID in queue for duplicate asset '{}': {}",
                        unit.image.path, err
                    )
                })?;

            Ok(PendingAlbumUnit {
                album_id: album_id.clone(),
                remote_id,
                unit: unit.clone(),
                uploaded_count: 0,
            })
        }
        Ok(None) => Err(format!(
            "Duplicate asset '{}' could not be resolved to a unique remote ID for album assignment",
            unit.image.path
        )),
        Err(err) => Err(format!(
            "Error querying remote ID for duplicate asset '{}': {}",
            unit.image.path, err
        )),
    }
}

/// Batches all pending units by `AlbumId`, invokes Immich's `add_assets_to_album` API,
/// updates database sync states, emits audit logs and UI events, and tracks results.
pub(crate) async fn assign_pending_album_units(
    client: &Arc<ImmichClient>,
    pool: &SqlitePool,
    app: &tauri::AppHandle,
    audit_context: &SyncAuditContext,
    pending_units: Vec<PendingAlbumUnit>,
    queue_results: &mut Vec<(String, String)>,
    uploaded: &mut usize,
) {
    if pending_units.is_empty() {
        return;
    }

    let mut by_album: BTreeMap<AlbumId, Vec<PendingAlbumUnit>> = BTreeMap::new();
    for item in pending_units {
        by_album.entry(item.album_id.clone()).or_default().push(item);
    }

    for (album_id, items) in by_album {
        let asset_ids: Vec<RemoteAssetId> = items.iter().map(|item| item.remote_id.clone()).collect();
        let album_result = client.add_assets_to_album(&album_id, &asset_ids).await;

        match album_result {
            Ok(()) => {
                for item in items {
                    let states = uploaded_asset_states(&item.unit);
                    let _ = db::persist_uploaded_unit(pool, &states, &item.remote_id).await;
                    for asset in item.unit.assets() {
                        queue_results.push((asset.path.clone(), "SYNCED".to_string()));
                    }
                    *uploaded += item.uploaded_count;
                    audit_event(
                        app,
                        audit_context.event(
                            "album.assets.assigned",
                            audit::Outcome::Success,
                            audit::Severity::Info,
                            "Assets assigned to target album",
                            json!({
                                "album_id": album_id.to_string(),
                                "remote_asset_id": item.remote_id.to_string(),
                                "local_path": item.unit.image.path,
                            }),
                        ),
                    );
                }
            }
            Err(error) => {
                let failure_reason = format!(
                    "Target album assignment failed for album '{}': {}",
                    album_id, error
                );
                for item in items {
                    log_to_ui_file_error(
                        app,
                        "ERROR",
                        &format!(
                            "Album assignment failed for '{}': {}",
                            item.unit.image.path, failure_reason
                        ),
                        "log_album_assignment_failed",
                        &item.unit.image.path,
                        &failure_reason,
                    );
                    audit_event(
                        app,
                        audit_context.event(
                            "album.assets.assignment.failed",
                            audit::Outcome::Failure,
                            audit::Severity::Error,
                            "Failed to assign asset to target album",
                            json!({
                                "album_id": album_id.to_string(),
                                "remote_asset_id": item.remote_id.to_string(),
                                "local_path": item.unit.image.path,
                                "failure_reason": audit_context.safe_error(&failure_reason),
                            }),
                        ),
                    );
                    for asset in item.unit.assets() {
                        let _ = db::mark_sync_failed(
                            pool,
                            &asset.path,
                            asset.mtime,
                            asset.size as i64,
                            &failure_reason,
                        )
                        .await;
                        let _ = db::update_sync_queue_failure(
                            pool,
                            &asset.path,
                            &failure_reason,
                        )
                        .await;
                        queue_results.push((asset.path.clone(), "FAILED".to_string()));
                    }
                }
            }
        }
    }
}
