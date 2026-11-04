//! Dedicated batching, assignment, duplicate resolution, and auditing for Immich target albums.

use serde_json::json;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{
    app::events::log_to_ui_file_error,
    audit::{self, audit_event, SyncAuditContext},
    immich::{AlbumId, BulkCheckDuplicate, ImmichClient, RemoteAssetId},
    media::model::HashedUploadUnit,
    sync::completion::{complete_album_assignment, PendingAlbumUnit},
};

/// Resolves the Remote Asset ID for a Duplicate Upload Unit.
///
/// Completion persists the returned ID before it can report the Upload Unit as synchronized.
pub(crate) async fn resolve_duplicate_remote_id(
    client: &ImmichClient,
    unit: &HashedUploadUnit,
    existing: &[BulkCheckDuplicate],
) -> Result<RemoteAssetId, String> {
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
        Ok(Some(remote_id)) => Ok(remote_id),
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
pub(crate) async fn assign_pending_album_units<R: tauri::Runtime>(
    client: &Arc<ImmichClient>,
    pool: &SqlitePool,
    app: &tauri::AppHandle<R>,
    audit_context: &SyncAuditContext,
    pending_units: Vec<PendingAlbumUnit>,
    queue_results: &mut Vec<(String, String)>,
    uploaded: &mut usize,
) -> Result<(), String> {
    if pending_units.is_empty() {
        return Ok(());
    }

    let mut by_album: BTreeMap<AlbumId, Vec<PendingAlbumUnit>> = BTreeMap::new();
    for item in pending_units {
        by_album
            .entry(item.album_id.clone())
            .or_default()
            .push(item);
    }

    for (album_id, items) in by_album {
        let asset_ids: Vec<RemoteAssetId> =
            items.iter().map(|item| item.remote_id.clone()).collect();
        let album_result = client.add_assets_to_album(&album_id, &asset_ids).await;

        match album_result {
            Ok(()) => {
                for item in items {
                    match complete_album_assignment(pool, item.clone()).await {
                        Ok(completion) => {
                            queue_results.extend_from_slice(completion.queue_results());
                            *uploaded += completion.uploaded_count();
                        }
                        Err(error) => {
                            log_to_ui_file_error(
                                app,
                                "ERROR",
                                &format!(
                                    "Could not durably complete album assignment for '{}': {error}",
                                    item.unit.image.path
                                ),
                                "log_album_assignment_persist_failed",
                                &item.unit.image.path,
                                &error,
                            );
                            audit_event(
                                app,
                                audit_context.event(
                                    "album.assets.assignment.persistence.failed",
                                    audit::Outcome::Failure,
                                    audit::Severity::Error,
                                    "Album Assignment succeeded but durable completion failed",
                                    json!({
                                        "album_id": album_id.to_string(),
                                        "remote_asset_id": item.remote_id.to_string(),
                                        "local_path": item.unit.image.path,
                                        "failure_reason": audit_context.safe_error(&error),
                                    }),
                                ),
                            );
                            mark_pending_unit_failed(pool, &item, &error, queue_results).await?;
                            continue;
                        }
                    }
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
                    mark_pending_unit_failed(pool, &item, &failure_reason, queue_results).await?;
                }
            }
        }
    }
    Ok(())
}

async fn mark_pending_unit_failed(
    pool: &SqlitePool,
    item: &PendingAlbumUnit,
    reason: &str,
    queue_results: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let failure = crate::sync::completion::fail_upload_unit(pool, &item.unit, reason).await?;
    queue_results.extend_from_slice(failure.queue_results());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::AuthConfig,
        db::{self, QueueAsset},
        media::model::{Checksums, SyncAsset},
        sync::completion::begin_upload_unit_completion,
    };
    use sqlx::{sqlite::SqlitePoolOptions, Row};
    use tokio::{io::AsyncWriteExt, net::TcpListener};

    async fn test_pool() -> SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn upload_unit(album_id: AlbumId) -> HashedUploadUnit {
        HashedUploadUnit {
            image: SyncAsset {
                path: "photo.jpg".to_string(),
                size: 123,
                checksums: Some(Checksums {
                    sha1_base64: "photo-hash".to_string(),
                }),
                mtime: 42,
                target_album_id: Some(album_id),
                remote_id: None,
            },
            live_photo_video: None,
        }
    }

    #[tokio::test]
    async fn assigns_a_pending_album_unit_then_records_its_terminal_completion() {
        let pool = test_pool().await;
        let album_id = AlbumId::new("album-1");
        db::enqueue_sync_assets(
            &pool,
            &[QueueAsset {
                path: "photo.jpg",
                size: 123,
                mtime: 42,
                target_album_id: Some(&album_id),
            }],
            &[],
        )
        .await
        .unwrap();
        let pending = begin_upload_unit_completion(
            &pool,
            upload_unit(album_id.clone()),
            RemoteAssetId::new("remote-photo"),
            1,
        )
        .await
        .unwrap()
        .into_pending_album()
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let response = r#"[{"id":"remote-photo","success":true}]"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.len(),
                        response
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let server_url = format!("http://{address}");
        let client = Arc::new(
            ImmichClient::new(server_url.clone(), "test-key".to_string(), true, None, None)
                .unwrap(),
        );
        let app = tauri::test::mock_app();
        let audit_context = SyncAuditContext::from_credentials(
            &AuthConfig {
                server_url,
                api_key: "test-key".to_string(),
                allow_local_http: true,
                cloudflare_client_id: None,
                cloudflare_client_secret: None,
            },
            "manual_sync",
        );
        let mut queue_results = Vec::new();
        let mut uploaded = 0;

        assign_pending_album_units(
            &client,
            &pool,
            app.handle(),
            &audit_context,
            vec![pending],
            &mut queue_results,
            &mut uploaded,
        )
        .await
        .unwrap();

        assert_eq!(
            queue_results,
            [("photo.jpg".to_string(), "SYNCED".to_string())]
        );
        assert_eq!(uploaded, 1);
        let state =
            sqlx::query("SELECT status, remote_id FROM sync_state WHERE local_path = 'photo.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state.get::<String, _>("status"), "SYNCED");
        assert_eq!(
            state.get::<Option<String>, _>("remote_id").as_deref(),
            Some("remote-photo")
        );
    }
}
