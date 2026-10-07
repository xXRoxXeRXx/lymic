//! Hashing, bulk-check, and upload processor implementation.

use crate::{
    app::events::log_to_ui,
    audit::{self, audit_event, upload_details, SyncAuditContext},
    db,
    immich::ImmichClient,
    media::{
        model::{upload_units_from_assets, HashedUploadUnit, ScanResult, UploadUnit},
        scanner::{calculate_stable_checksums, reuse_cached_checksums},
    },
    sync::{retry::check_assets_exist_with_backoff, JobByteProgress, SyncCoordinatorInner},
};
use futures::{stream::FuturesUnordered, StreamExt};
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};
use tauri::Emitter;

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSummary {
    pub(crate) processed: usize,
    pub(crate) uploaded: usize,
    pub(crate) failed: usize,
    #[serde(skip)]
    pub(crate) queue_results: Vec<(String, String)>,
}

pub(crate) const BULK_CHECK_CHUNK_SIZE: usize = 100;

pub(crate) async fn process_sync_block(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: Arc<ImmichClient>,
    scan_result: ScanResult,
    audit_context: SyncAuditContext,
    progress: JobByteProgress,
    coordinator: Arc<SyncCoordinatorInner>,
) -> Result<SyncSummary, String> {
    let upload_parallelism = load_upload_parallelism(&pool).await?;
    let ScanResult {
        files, failures, ..
    } = scan_result;
    let mut queue_results = Vec::new();
    for failure in failures {
        log_to_ui(
            &app,
            "ERROR",
            &format!("{}: {}", failure.path, failure.error),
        );
        let _ = db::mark_sync_failed(&pool, &failure.path, 0, 0, &failure.error).await;
        queue_results.push((failure.path, "FAILED".to_string()));
    }
    let units = upload_units_from_assets(files);

    let mut hashed_units = Vec::new();
    let mut hashing = FuturesUnordered::new();
    for unit in units {
        if !can_start_work(&coordinator) {
            break;
        }
        hashing.push(hash_unit(
            unit,
            pool.clone(),
            app.clone(),
            progress.clone(),
            audit_context.clone(),
        ));
        if hashing.len() == 4 {
            if let Some(outcome) = hashing.next().await {
                queue_results.extend(outcome.queue_results);
                if let Some(unit) = outcome.unit {
                    hashed_units.push(unit);
                }
            }
        }
    }
    while let Some(outcome) = hashing.next().await {
        queue_results.extend(outcome.queue_results);
        if let Some(unit) = outcome.unit {
            hashed_units.push(unit);
        }
    }

    let mut uploaded = 0;
    for chunk in hashed_units.chunks(BULK_CHECK_CHUNK_SIZE) {
        if !can_start_work(&coordinator) {
            break;
        }
        let hashes = chunk
            .iter()
            .flat_map(|unit| unit.assets())
            .filter_map(|asset| {
                asset
                    .checksums
                    .as_ref()
                    .map(|checksums| checksums.sha1_base64.clone())
            })
            .collect();
        let existing =
            check_assets_exist_with_backoff(&client, hashes, &app, &audit_context).await?;
        // A check in flight may complete after pause, but its answer must not start work.
        if !can_start_work(&coordinator) {
            break;
        }
        let mut upload_units = Vec::new();
        for unit in chunk {
            let accepted: Vec<_> = unit
                .assets()
                .filter(|asset| !existing.contains(&asset.checksums.as_ref().unwrap().sha1_base64))
                .cloned()
                .collect();
            for asset in unit
                .assets()
                .filter(|asset| existing.contains(&asset.checksums.as_ref().unwrap().sha1_base64))
            {
                let hash = &asset.checksums.as_ref().unwrap().sha1_base64;
                let status = if db::update_sync_state(
                    &pool,
                    &asset.path,
                    hash,
                    asset.mtime,
                    asset.size as i64,
                    "SYNCED",
                    None,
                )
                .await
                .is_ok()
                {
                    "SYNCED"
                } else {
                    let _ = db::mark_sync_failed(
                        &pool,
                        &asset.path,
                        asset.mtime,
                        asset.size as i64,
                        "Could not persist synced state",
                    )
                    .await;
                    "FAILED"
                };
                queue_results.push((asset.path.clone(), status.to_string()));
                progress.advance(&app, asset.size);
            }
            if let Some(image) = accepted.first() {
                upload_units.push(HashedUploadUnit {
                    image: image.clone(),
                    live_photo_video: accepted.get(1).cloned(),
                });
            }
        }
        let mut pending = upload_units.into_iter();
        let mut uploads = FuturesUnordered::new();
        while can_start_work(&coordinator) && uploads.len() < upload_parallelism {
            let Some(unit) = pending.next() else {
                break;
            };
            uploads.push(upload_unit(
                unit,
                client.clone(),
                pool.clone(),
                app.clone(),
                progress.clone(),
                audit_context.clone(),
            ));
        }
        while let Some(outcome) = uploads.next().await {
            uploaded += outcome.uploaded;
            queue_results.extend(outcome.queue_results);
            if can_start_work(&coordinator) {
                if let Some(unit) = pending.next() {
                    uploads.push(upload_unit(
                        unit,
                        client.clone(),
                        pool.clone(),
                        app.clone(),
                        progress.clone(),
                        audit_context.clone(),
                    ));
                }
            }
        }
        if !can_start_work(&coordinator) {
            break;
        }
    }

    let failed = queue_results
        .iter()
        .filter(|(_, status)| status == "FAILED")
        .count();
    let processed = queue_results.len();
    Ok(SyncSummary {
        processed,
        uploaded,
        failed,
        queue_results,
    })
}

fn can_start_work(coordinator: &SyncCoordinatorInner) -> bool {
    !coordinator.paused.load(Ordering::SeqCst)
}

struct HashOutcome {
    unit: Option<HashedUploadUnit>,
    queue_results: Vec<(String, String)>,
}

async fn hash_unit(
    unit: UploadUnit,
    pool: sqlx::SqlitePool,
    app: tauri::AppHandle,
    progress: JobByteProgress,
    audit_context: SyncAuditContext,
) -> HashOutcome {
    let result: Result<HashedUploadUnit, String> = async {
        let mut hashed = Vec::new();
        for asset in unit.assets() {
            let cached = db::get_cached_hash(&pool, &asset.path, asset.mtime, asset.size as i64)
                .await
                .map_err(|error| {
                    format!("Could not load cached hash for {}: {}", asset.path, error)
                })?;
            if let Some(cached_sha1_base64) = cached.as_ref() {
                let path = asset.path.clone();
                let expected_size = asset.size;
                let expected_mtime = asset.mtime;
                let cached_sha1_base64 = cached_sha1_base64.clone();
                if let Some((checksums, size, mtime)) = tokio::task::spawn_blocking(move || {
                    reuse_cached_checksums(
                        &path,
                        expected_size,
                        expected_mtime,
                        &cached_sha1_base64,
                    )
                })
                .await
                .map_err(|error| format!("Hashing task failed: {}", error))?
                .map_err(|error| format!("Hashing failed: {}", error))?
                {
                    hashed.push(crate::media::model::SyncAsset {
                        path: asset.path.clone(),
                        size,
                        mtime,
                        checksums: Some(checksums),
                    });
                    continue;
                }
            }
            let path = asset.path.clone();
            let (checksums, size, mtime) =
                tokio::task::spawn_blocking(move || calculate_stable_checksums(&path))
                    .await
                    .map_err(|error| format!("Hashing task failed: {}", error))?
                    .map_err(|error| format!("Hashing failed: {}", error))?;
            if cached.as_deref() != Some(&checksums.sha1_base64) {
                db::update_sync_state(
                    &pool,
                    &asset.path,
                    &checksums.sha1_base64,
                    mtime,
                    size as i64,
                    "PENDING",
                    None,
                )
                .await
                .map_err(|error| {
                    format!(
                        "Could not persist pending sync state for {}: {}",
                        asset.path, error
                    )
                })?;
            }
            hashed.push(crate::media::model::SyncAsset {
                path: asset.path.clone(),
                size,
                mtime,
                checksums: Some(checksums),
            });
        }
        Ok(HashedUploadUnit {
            image: hashed.remove(0),
            live_photo_video: hashed.pop(),
        })
    }
    .await;

    match result {
        Ok(unit) => HashOutcome {
            unit: Some(unit),
            queue_results: Vec::new(),
        },
        Err(error) => {
            let paths: Vec<_> = unit.assets().map(|asset| asset.path.clone()).collect();
            for asset in unit.assets() {
                let _ = db::mark_sync_failed(
                    &pool,
                    &asset.path,
                    asset.mtime,
                    asset.size as i64,
                    &error,
                )
                .await;
            }
            log_to_ui(&app, "ERROR", &error);
            audit_event(&app, audit_context.event("file.hash.failed", audit::Outcome::Failure, audit::Severity::Error, "File hashing failed", json!({ "local_paths": paths, "failure_reason": audit_context.safe_error(&error) })));
            progress.advance(&app, unit.assets().map(|asset| asset.size).sum());
            HashOutcome {
                unit: None,
                queue_results: unit
                    .assets()
                    .map(|asset| (asset.path.clone(), "FAILED".to_string()))
                    .collect(),
            }
        }
    }
}

struct UploadOutcome {
    uploaded: usize,
    queue_results: Vec<(String, String)>,
}

async fn upload_unit(
    unit: HashedUploadUnit,
    client: Arc<ImmichClient>,
    pool: sqlx::SqlitePool,
    app: tauri::AppHandle,
    progress: JobByteProgress,
    audit_context: SyncAuditContext,
) -> UploadOutcome {
    let image_hash = unit.image.checksums.as_ref().unwrap().sha1_base64.clone();
    let video_path = unit
        .live_photo_video
        .as_ref()
        .map(|asset| asset.path.as_str());
    let operation_id = audit::AuditEvent::operation_id();
    let started = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let mut details = upload_details(
        &unit.image,
        unit.image.checksums.as_ref().unwrap(),
        &operation_id,
        &started,
        None,
        None,
        None,
    );
    if let Some(video) = &unit.live_photo_video {
        details["live_photo_local_path"] = json!(video.path);
    }
    audit_event(
        &app,
        audit_context.event(
            "file.upload.started",
            audit::Outcome::Started,
            audit::Severity::Info,
            "Upload started",
            details,
        ),
    );
    let _ = app.emit("sync-progress", &unit.image.path);

    let (uploaded, status, failure_reason, remote_id) = match client
        .upload_asset_with_live_photo(&unit.image.path, &image_hash, video_path)
        .await
    {
        Ok(remote_id) => {
            let mut persisted = true;
            for asset in unit.assets() {
                let hash = &asset.checksums.as_ref().unwrap().sha1_base64;
                let remote = if asset.path == unit.image.path {
                    Some(remote_id.as_str())
                } else {
                    None
                };
                if db::update_sync_state(
                    &pool,
                    &asset.path,
                    hash,
                    asset.mtime,
                    asset.size as i64,
                    "SYNCED",
                    remote,
                )
                .await
                .is_err()
                {
                    persisted = false;
                }
            }
            if persisted {
                (unit.assets().count(), "SYNCED", None, Some(remote_id))
            } else {
                let reason = "Upload completed but sync state could not be persisted atomically";
                for asset in unit.assets() {
                    let _ = db::mark_sync_failed(
                        &pool,
                        &asset.path,
                        asset.mtime,
                        asset.size as i64,
                        reason,
                    )
                    .await;
                }
                (0, "FAILED", Some(reason.to_string()), None)
            }
        }
        Err(error) => {
            log_to_ui(
                &app,
                "ERROR",
                &format!("Upload failed for {}: {}", unit.image.path, error),
            );
            for asset in unit.assets() {
                let _ = db::mark_sync_failed(
                    &pool,
                    &asset.path,
                    asset.mtime,
                    asset.size as i64,
                    &error,
                )
                .await;
            }
            (0, "FAILED", Some(error), None)
        }
    };
    let mut details = upload_details(
        &unit.image,
        unit.image.checksums.as_ref().unwrap(),
        &operation_id,
        &started,
        Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)),
        remote_id.as_deref(),
        failure_reason
            .as_deref()
            .map(|error| audit_context.safe_error(error)),
    );
    if let Some(video) = &unit.live_photo_video {
        details["live_photo_local_path"] = json!(video.path);
    }
    audit_event(
        &app,
        audit_context.event(
            "file.upload.completed",
            if status == "SYNCED" {
                audit::Outcome::Success
            } else {
                audit::Outcome::Failure
            },
            if status == "SYNCED" {
                audit::Severity::Info
            } else {
                audit::Severity::Error
            },
            "Upload completed",
            details,
        ),
    );
    progress.advance(&app, unit.byte_size());
    UploadOutcome {
        uploaded,
        queue_results: unit
            .assets()
            .map(|asset| (asset.path.clone(), status.to_string()))
            .collect(),
    }
}

pub(crate) async fn load_upload_parallelism(pool: &sqlx::SqlitePool) -> Result<usize, String> {
    db::get_upload_parallelism(pool)
        .await
        .map_err(|error| format!("Could not load upload parallelism: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pipeline_upload_parallelism_loader_uses_the_configured_value() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{}?mode=memory&cache=private",
                database_name
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert_eq!(load_upload_parallelism(&pool).await.unwrap(), 3);
        db::set_upload_parallelism(&pool, 8).await.unwrap();
        assert_eq!(load_upload_parallelism(&pool).await.unwrap(), 8);
    }

    #[test]
    fn upload_audit_details_include_only_immich_checksum() {
        let asset = crate::media::model::SyncAsset {
            path: "C:/media/photo.jpg".to_string(),
            size: 123,
            checksums: None,
            mtime: 0,
        };
        let checksums = crate::media::model::Checksums {
            sha1_base64: "c2hhMQ==".to_string(),
        };
        let details = upload_details(
            &asset,
            &checksums,
            "file-operation",
            "2026-10-04T00:00:00.000000000Z",
            Some("2026-10-04T00:00:01.000000000Z".to_string()),
            Some("remote-id"),
            None,
        );
        assert_eq!(details["file_size_bytes"], 123);
        assert_eq!(details["checksums"]["sha1_base64"], checksums.sha1_base64);
        assert!(details["checksums"].get("md5").is_none());
        assert!(details["checksums"].get("sha256").is_none());
        assert_eq!(details["remote_asset_id"], "remote-id");
        assert!(details.get("failure_reason").is_none());
    }

    #[test]
    fn bulk_check_chunk_size_is_bounded_to_100() {
        assert_eq!(BULK_CHECK_CHUNK_SIZE, 100);
    }

    #[test]
    fn pause_before_hashing_prevents_new_hashes() {
        let coordinator = SyncCoordinatorInner {
            lock: tokio::sync::Mutex::new(()),
            paused: std::sync::atomic::AtomicBool::new(true),
            resume: tokio::sync::Notify::new(),
        };
        assert!(!can_start_work(&coordinator));
    }

    #[test]
    fn pause_during_hashing_prevents_follow_up_hashes() {
        let coordinator = SyncCoordinatorInner {
            lock: tokio::sync::Mutex::new(()),
            paused: std::sync::atomic::AtomicBool::new(false),
            resume: tokio::sync::Notify::new(),
        };
        assert!(can_start_work(&coordinator));
        coordinator.paused.store(true, Ordering::SeqCst);
        assert!(!can_start_work(&coordinator));
    }

    #[test]
    fn pause_before_bulk_check_prevents_new_request() {
        let coordinator = SyncCoordinatorInner {
            lock: tokio::sync::Mutex::new(()),
            paused: std::sync::atomic::AtomicBool::new(true),
            resume: tokio::sync::Notify::new(),
        };
        assert!(!can_start_work(&coordinator));
    }

    #[test]
    fn pause_during_uploads_prevents_follow_up_uploads() {
        let coordinator = SyncCoordinatorInner {
            lock: tokio::sync::Mutex::new(()),
            paused: std::sync::atomic::AtomicBool::new(false),
            resume: tokio::sync::Notify::new(),
        };
        assert!(can_start_work(&coordinator));
        coordinator.paused.store(true, Ordering::SeqCst);
        assert!(!can_start_work(&coordinator));
    }
}
