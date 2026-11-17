//! Dedicated batching, assignment, duplicate resolution, and auditing for Immich target albums.

use serde_json::json;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{
    app::events::{log_to_ui_file_error, log_to_ui_keyed},
    audit::{self, audit_event, SyncAuditContext},
    immich::{AlbumId, BulkCheckDuplicate, DuplicateRemoteAsset, ImmichClient, RemoteAssetId},
    media::model::HashedUploadUnit,
    sync::completion::{complete_album_assignment, PendingAlbumUnit},
};

/// Resolves the Remote Asset of a Duplicate Upload Unit, including one in the Immich trash:
/// the one the bulk check named, otherwise the one a checksum search finds.
///
/// Completion persists the returned ID before it can report the Upload Unit as synchronized.
pub(crate) async fn resolve_duplicate_remote_asset(
    client: &ImmichClient,
    unit: &HashedUploadUnit,
    existing: &[BulkCheckDuplicate],
) -> Result<DuplicateRemoteAsset, String> {
    let image_hash = &unit
        .image
        .checksums
        .as_ref()
        .expect("hashed unit has checksums")
        .sha1_base64;

    let resolved = match existing
        .iter()
        .find(|d| &d.hash == image_hash)
        .and_then(|d| d.remote_asset.clone())
    {
        Some(found) => Ok(Some(found)),
        None => client.search_asset_by_checksum(image_hash).await,
    };

    match resolved {
        Ok(Some(found)) => Ok(found),
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

/// Reports a Trashed Duplicate: the Upload Unit counts as synchronized, but its Remote
/// Asset stays in the Immich trash. Lymic never re-uploads or restores it.
pub(crate) fn report_trashed_duplicate<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    audit_context: &SyncAuditContext,
    unit: &HashedUploadUnit,
    remote_id: &RemoteAssetId,
) {
    log_to_ui_keyed(
        app,
        "WARN",
        &format!(
            "'{}' is already in Immich, but in the Immich trash; it was not uploaded again",
            unit.image.path
        ),
        "log_duplicate_in_trash",
        [("file".to_string(), unit.image.path.clone())]
            .into_iter()
            .collect(),
    );
    audit_event(
        app,
        audit_context.event(
            "sync.duplicate.trashed",
            audit::Outcome::Success,
            audit::Severity::Warn,
            "Duplicate Remote Asset is in the Immich trash; left there",
            json!({
                "remote_asset_id": remote_id.to_string(),
                "local_path": unit.image.path,
            }),
        ),
    );
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
    use crate::db::test_pool;
    use crate::{
        auth::AuthConfig,
        db::{self, QueueAsset},
        media::model::{Checksums, SyncAsset},
        sync::completion::begin_upload_unit_completion,
    };
    use sqlx::Row;
    use tokio::{io::AsyncWriteExt, net::TcpListener};

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

    #[tokio::test]
    async fn failed_target_album_assignment_stays_failed_with_its_persisted_remote_id_for_retry() {
        let pool = test_pool().await;
        let album_id = AlbumId::new("deleted-album");
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
            upload_unit(album_id),
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
            let response = r#"{"message":"Album not found"}"#;
            stream.write_all(format!("HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
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
            [("photo.jpg".to_string(), "FAILED".to_string())]
        );
        assert_eq!(uploaded, 0);
        let queue =
            sqlx::query("SELECT status, remote_id FROM sync_queue WHERE local_path = 'photo.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queue.get::<String, _>("status"), "FAILED");
        assert_eq!(
            queue.get::<Option<String>, _>("remote_id").as_deref(),
            Some("remote-photo")
        );
    }

    fn audit_context() -> SyncAuditContext {
        SyncAuditContext::from_credentials(
            &AuthConfig::test_fixture("http://127.0.0.1"),
            "manual_sync",
        )
    }

    #[tokio::test]
    async fn a_duplicate_reported_trashed_by_the_bulk_check_resolves_to_its_trashed_remote_asset() {
        // No server: the bulk check already named the Remote Asset, so nothing is searched.
        let client =
            crate::immich::test_support::test_client("http://127.0.0.1:9".to_string(), "test-key");
        let existing = [BulkCheckDuplicate {
            hash: "photo-hash".to_string(),
            remote_asset: Some(DuplicateRemoteAsset {
                id: RemoteAssetId::new("trashed-id"),
                is_trashed: true,
            }),
        }];

        let resolved = resolve_duplicate_remote_asset(
            &client,
            &upload_unit(AlbumId::new("album-1")),
            &existing,
        )
        .await
        .unwrap();

        assert_eq!(
            resolved,
            DuplicateRemoteAsset {
                id: RemoteAssetId::new("trashed-id"),
                is_trashed: true,
            }
        );
    }

    #[tokio::test]
    async fn a_duplicate_without_an_asset_id_finds_its_trashed_remote_asset_by_checksum() {
        let (url, request) = crate::immich::test_support::serve_once(
            "200 OK",
            r#"{"assets":{"items":[{"id":"trashed-id","isTrashed":true}]}}"#,
        )
        .await;
        let client = crate::immich::test_support::test_client(url, "test-key");
        let existing = [BulkCheckDuplicate {
            hash: "photo-hash".to_string(),
            remote_asset: None,
        }];

        let resolved = resolve_duplicate_remote_asset(
            &client,
            &upload_unit(AlbumId::new("album-1")),
            &existing,
        )
        .await
        .unwrap();

        assert!(request.await.unwrap().contains(r#""withDeleted":true"#));
        assert_eq!(
            resolved,
            DuplicateRemoteAsset {
                id: RemoteAssetId::new("trashed-id"),
                is_trashed: true,
            }
        );
    }

    #[tokio::test]
    async fn a_trashed_duplicate_is_reported_as_in_the_immich_trash() {
        let app = tauri::test::mock_app();
        let logs = crate::app::events::CapturedUiLogs::listen(app.handle());

        report_trashed_duplicate(
            app.handle(),
            &audit_context(),
            &upload_unit(AlbumId::new("album-1")),
            &RemoteAssetId::new("trashed-id"),
        );

        // The mock app has no audit logger, so it also logs `log_audit_failure`.
        let reports = logs.with_key("log_duplicate_in_trash");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["level"], "WARN");
        assert_eq!(reports[0]["params"]["file"], "photo.jpg");
    }
}
