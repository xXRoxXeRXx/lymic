//! Hashing, bulk-check, and upload processor implementation.

use crate::{
    app::events::log_to_ui,
    audit::{self, audit_event, upload_details, SyncAuditContext},
    db,
    immich::ImmichClient,
    media::{
        model::{upload_units_from_assets, Asset, HashedUploadUnit, ScanResult},
        scanner::calculate_stable_checksums,
    },
    sync::retry::check_assets_exist_with_backoff,
};
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use tauri::Emitter;

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSummary {
    pub(crate) processed: usize,
    pub(crate) uploaded: usize,
    pub(crate) failed: usize,
}

pub(crate) async fn process_sync_block(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: Arc<ImmichClient>,
    scan_result: ScanResult,
    audit_context: SyncAuditContext,
) -> Result<SyncSummary, String> {
    let upload_parallelism = load_upload_parallelism(&pool).await?;
    let ScanResult {
        files, failures, ..
    } = scan_result;
    let total_files = files.len();
    let total_bytes: u64 = files.iter().map(|asset| asset.size).sum();
    let scan_failure_count = failures.len();
    let completed_bytes = Arc::new(AtomicU64::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(failures.len()));
    for failure in failures {
        log_to_ui(
            &app,
            "ERROR",
            &format!("{}: {}", failure.path, failure.error),
        );
        let _ = db::mark_sync_failed(&pool, &failure.path, 0, 0, &failure.error).await;
    }
    let units = upload_units_from_assets(files);
    log_to_ui(
        &app,
        "INFO",
        &format!(
            "{}: Starting pipeline for {} files ({:.2} MB)",
            "Sync",
            total_files,
            total_bytes as f64 / 1024.0 / 1024.0
        ),
    );

    // Each unit contains at most two assets, so 250 units keeps a bulk request at
    // or below 500 hashes while keeping live-photo pairs together.
    let hashed_units = futures::stream::iter(units)
        .map(|unit| {
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let failure_count = failure_count.clone();
            let audit_context = audit_context.clone();
            async move {
                let result: Result<HashedUploadUnit, String> = async {
                    let mut hashed = Vec::new();
                    let assets: Vec<Asset> = unit.assets().cloned().collect();
                    for asset in &assets {
                        let cached = if asset.mtime == 0 { Ok(None) } else { db::get_cached_hash(&pool, &asset.path, asset.mtime, asset.size as i64).await };
                        let cached = match cached {
                            Ok(value) => value,
                            Err(error) => return Err(format!("Could not load cached hash for {}: {}", asset.path, error)),
                        };
                        let path = asset.path.clone();
                        let (checksums, size, mtime) = tokio::task::spawn_blocking(move || calculate_stable_checksums(&path)).await.map_err(|error| format!("Hashing task failed: {}", error))?.map_err(|error| format!("Hashing failed: {}", error))?;
                        if cached.as_deref() != Some(&checksums.sha1_base64) {
                            db::update_sync_state(&pool, &asset.path, &checksums.sha1_base64, mtime, size as i64, "PENDING", None).await.map_err(|error| format!("Could not persist pending sync state for {}: {}", asset.path, error))?;
                        }
                        hashed.push(crate::media::model::SyncAsset { path: asset.path.clone(), size, mtime, checksums: Some(checksums) });
                    }
                    Ok(HashedUploadUnit { image: hashed.remove(0), live_photo_video: hashed.pop() })
                }.await;
                match result {
                    Ok(unit) => Some(unit),
                    Err(error) => {
                        for asset in unit.assets() { let _ = db::mark_sync_failed(&pool, &asset.path, asset.mtime, asset.size as i64, &error).await; }
                        log_to_ui(&app, "ERROR", &error);
                        audit_event(&app, audit_context.event("file.hash.failed", audit::Outcome::Failure, audit::Severity::Error, "File hashing failed", json!({ "local_paths": unit.assets().map(|asset| asset.path.clone()).collect::<Vec<_>>(), "failure_reason": audit_context.safe_error(&error) })));
                        failure_count.fetch_add(unit.assets().count(), Ordering::Relaxed);
                        completed_bytes.fetch_add(unit.assets().map(|asset| asset.size).sum(), Ordering::SeqCst);
                        None
                    }
                }
            }
        })
        .buffer_unordered(4)
        .filter_map(|unit| async { unit })
        .chunks(250)
        .then(|chunk| {
            let client = client.clone();
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let failure_count = failure_count.clone();
            let audit_context = audit_context.clone();
            async move {
                let hashes = chunk.iter().flat_map(|unit| unit.assets()).filter_map(|asset| asset.checksums.as_ref().map(|checksums| checksums.sha1_base64.clone())).collect();
                let existing = check_assets_exist_with_backoff(&client, hashes, &app, &audit_context).await?;
                let mut upload_units = Vec::new();
                for unit in chunk {
                    let accepted: Vec<_> = unit.assets().filter(|asset| !existing.contains(&asset.checksums.as_ref().unwrap().sha1_base64)).cloned().collect();
                    for asset in unit.assets().filter(|asset| existing.contains(&asset.checksums.as_ref().unwrap().sha1_base64)) {
                        let hash = &asset.checksums.as_ref().unwrap().sha1_base64;
                        if db::update_sync_state(&pool, &asset.path, hash, asset.mtime, asset.size as i64, "SYNCED", None).await.is_err() { failure_count.fetch_add(1, Ordering::Relaxed); }
                        completed_bytes.fetch_add(asset.size, Ordering::SeqCst);
                    }
                    if let Some(image) = accepted.first() { upload_units.push(HashedUploadUnit { image: image.clone(), live_photo_video: accepted.get(1).cloned() }); }
                }
                Ok::<_, String>(upload_units)
            }
        });

    hashed_units
        .map_ok(|units| futures::stream::iter(units.into_iter().map(Ok::<_, String>)))
        .try_flatten()
        // The limit is loaded once per sync so an active run remains stable.
        .try_for_each_concurrent(upload_parallelism, |unit| {
            let client = client.clone();
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let success_count = success_count.clone();
            let failure_count = failure_count.clone();
            let audit_context = audit_context.clone();
            async move {
                let image_hash = unit.image.checksums.as_ref().unwrap().sha1_base64.clone();
                let video_path = unit.live_photo_video.as_ref().map(|asset| asset.path.as_str());
                let operation_id = audit::AuditEvent::operation_id();
                let started = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
                let mut details = upload_details(&unit.image, unit.image.checksums.as_ref().unwrap(), &operation_id, &started, None, None, None);
                if let Some(video) = &unit.live_photo_video { details["live_photo_local_path"] = json!(video.path); }
                audit_event(&app, audit_context.event("file.upload.started", audit::Outcome::Started, audit::Severity::Info, "Upload started", details));
                let _ = app.emit("sync-progress", &unit.image.path);
                let result = client.upload_asset_with_live_photo(&unit.image.path, &image_hash, video_path).await;
                match result {
                    Ok(remote_id) => {
                        let mut persisted = true;
                        for asset in unit.assets() {
                            let hash = &asset.checksums.as_ref().unwrap().sha1_base64;
                            let remote = if asset.path == unit.image.path { Some(remote_id.as_str()) } else { None };
                            if db::update_sync_state(&pool, &asset.path, hash, asset.mtime, asset.size as i64, "SYNCED", remote).await.is_err() { persisted = false; }
                        }
                        if persisted { success_count.fetch_add(unit.assets().count(), Ordering::SeqCst); } else {
                            for asset in unit.assets() { let _ = db::mark_sync_failed(&pool, &asset.path, asset.mtime, asset.size as i64, "Upload completed but sync state could not be persisted atomically").await; }
                            failure_count.fetch_add(unit.assets().count(), Ordering::Relaxed);
                        }
                        let mut details = upload_details(&unit.image, unit.image.checksums.as_ref().unwrap(), &operation_id, &started, Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)), Some(&remote_id), None);
                        if let Some(video) = &unit.live_photo_video { details["live_photo_local_path"] = json!(video.path); }
                        audit_event(&app, audit_context.event("file.upload.completed", audit::Outcome::Success, audit::Severity::Info, "Upload completed", details));
                    }
                    Err(error) => {
                        log_to_ui(&app, "ERROR", &format!("Upload failed for {}: {}", unit.image.path, error));
                        for asset in unit.assets() { let _ = db::mark_sync_failed(&pool, &asset.path, asset.mtime, asset.size as i64, &error).await; }
                        failure_count.fetch_add(unit.assets().count(), Ordering::Relaxed);
                        let mut details = upload_details(&unit.image, unit.image.checksums.as_ref().unwrap(), &operation_id, &started, Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)), None, Some(audit_context.safe_error(&error)));
                        if let Some(video) = &unit.live_photo_video { details["live_photo_local_path"] = json!(video.path); }
                        audit_event(&app, audit_context.event("file.upload.completed", audit::Outcome::Failure, audit::Severity::Error, "Upload failed", details));
                    }
                }
                let done = completed_bytes.fetch_add(unit.byte_size(), Ordering::SeqCst) + unit.byte_size();
                let pct = if total_bytes == 0 { 100 } else { ((done as f64 / total_bytes as f64 * 100.0) as u32).min(100) };
                let _ = app.emit("sync-progress-percent", pct);
                Ok::<_, String>(())
            }
        })
        .await?;

    let uploaded = success_count.load(Ordering::SeqCst);
    let failed = failure_count.load(Ordering::SeqCst);
    Ok(SyncSummary {
        processed: total_files.saturating_sub(failed.saturating_sub(scan_failure_count)),
        uploaded,
        failed,
    })
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
    fn upload_audit_details_include_all_required_checksums() {
        let asset = crate::media::model::SyncAsset {
            path: "C:/media/photo.jpg".to_string(),
            size: 123,
            checksums: None,
            mtime: 0,
        };
        let checksums = crate::media::model::Checksums {
            md5_hex: "a".repeat(32),
            sha256_hex: "b".repeat(64),
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
        assert_eq!(details["checksums"]["md5"], checksums.md5_hex);
        assert_eq!(details["checksums"]["sha256"], checksums.sha256_hex);
        assert_eq!(details["checksums"]["sha1_base64"], checksums.sha1_base64);
        assert_eq!(details["remote_asset_id"], "remote-id");
        assert!(details.get("failure_reason").is_none());
    }
}
