//! Durable synchronization queue execution.

use crate::{
    app::{
        events::send_notification_keyed,
        locale::{backend_translations, LocaleState},
    },
    audit::{self, audit_event, SyncAuditContext},
    db,
    immich::{ImmichClient, SidecarDirectoryCache},
    media::model::ScanResult,
    sync::{
        processor::{RunProcessor, SyncSummary},
        JobByteProgress, SyncCoordinatorInner,
    },
};
use serde_json::json;
use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager};

const SYNC_QUEUE_BLOCK_SIZE: i64 = 50;

fn excluded_paths_at_queue_boundary(
    queued: &[db::QueuedUploadUnit],
    folders: &[db::WatchedFolder],
) -> Vec<String> {
    queued
        .iter()
        .filter(|unit| {
            std::iter::once(&unit.image)
                .chain(unit.live_photo_video.iter())
                .any(|asset| {
                    crate::media::scanner::path_matches_watched_folder_exclusion(
                        Path::new(&asset.local_path),
                        folders,
                    )
                })
        })
        .flat_map(|unit| {
            std::iter::once(&unit.image.local_path)
                .chain(unit.live_photo_video.iter().map(|asset| &asset.local_path))
        })
        .cloned()
        .collect()
}

pub(crate) async fn run_sync_pipeline(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: Arc<ImmichClient>,
    scan_result: ScanResult,
    is_auto: bool,
    coordinator: Arc<SyncCoordinatorInner>,
    audit_context: SyncAuditContext,
) -> Result<SyncSummary, String> {
    // Scans are merely producers. The durable queue is the sole source used by the
    // runner, including after recovery, so no completed item is re-scanned to resume.
    let folders = db::get_folders(&pool).await.unwrap_or_default();
    let (queue_units, queue_failures) =
        crate::sync::service::prepare_scan_for_queue(&scan_result, &folders);
    let snapshot = db::enqueue_upload_units(&pool, &queue_units, &queue_failures)
        .await
        .map_err(|error| format!("Could not persist sync queue: {}", error))?;
    crate::app::tray::update_sync_action(&app, &snapshot);
    let _sync_guard = match coordinator.lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if !is_auto {
                crate::app::events::log_to_ui_key(
                    &app,
                    "INFO",
                    "Sync queued, waiting for running synchronization.",
                    "log_sync_queued_waiting",
                );
            }
            coordinator.lock.lock().await
        }
    };
    let _ = app.emit("sync-started", ());
    let _ = app.emit("sync-progress-snapshot", &snapshot);
    audit_event(
        &app,
        audit_context.event(
            "sync.started",
            audit::Outcome::Started,
            audit::Severity::Info,
            "Synchronization started",
            json!({ "automatic": is_auto }),
        ),
    );
    let mut totals = SyncSummary {
        failed: snapshot.failed.max(0) as usize,
        ..Default::default()
    };
    let progress = JobByteProgress::new(
        snapshot.total_bytes.max(0) as u64,
        snapshot.completed_bytes.max(0) as u64,
    );
    let transfer_stats = progress.transfer_stats();
    let sidecar_cache = SidecarDirectoryCache::new();
    let processor = RunProcessor::new(
        app.clone(),
        pool.clone(),
        client,
        audit_context.clone(),
        progress.clone(),
        coordinator.clone(),
        sidecar_cache,
    )
    .await?;
    transfer_stats.set_active(true);
    let _transfer_cleanup = TransferStatsCleanup {
        app: app.clone(),
        tracker: transfer_stats.clone(),
    };
    spawn_transfer_reporter(app.clone(), transfer_stats.clone());
    emit_transfer_stats(&app, &transfer_stats);
    loop {
        // The durable queue is materialized in a small window. A pause is observed
        // before loading another window and inside the processor between new work.
        while coordinator.paused.load(Ordering::SeqCst) {
            let snapshot = db::set_sync_status(&pool, "PAUSED")
                .await
                .map_err(|error| format!("Could not persist paused sync: {}", error))?;
            crate::app::tray::update_sync_action(&app, &snapshot);
            transfer_stats.sync_queue(
                snapshot.total_bytes.max(0) as u64,
                snapshot.completed_bytes.max(0) as u64,
            );
            transfer_stats.set_paused(true);
            emit_transfer_stats(&app, &transfer_stats);
            let _ = app.emit("sync-paused", &snapshot);
            audit_event(
                &app,
                audit_context.event(
                    "sync.paused",
                    audit::Outcome::Info,
                    audit::Severity::Info,
                    "Synchronization paused",
                    json!({}),
                ),
            );
            coordinator.resume.notified().await;
            if !coordinator.paused.load(Ordering::SeqCst) {
                let snapshot = db::set_sync_status(&pool, "RUNNING")
                    .await
                    .map_err(|error| format!("Could not resume sync: {}", error))?;
                crate::app::tray::update_sync_action(&app, &snapshot);
                transfer_stats.sync_queue(
                    snapshot.total_bytes.max(0) as u64,
                    snapshot.completed_bytes.max(0) as u64,
                );
                transfer_stats.set_paused(false);
                emit_transfer_stats(&app, &transfer_stats);
                let _ = app.emit("sync-resumed", &snapshot);
                audit_event(
                    &app,
                    audit_context.event(
                        "sync.resumed",
                        audit::Outcome::Info,
                        audit::Severity::Info,
                        "Synchronization resumed",
                        json!({}),
                    ),
                );
            }
        }
        let queued = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .map_err(|error| format!("Could not load sync queue: {}", error))?;
        if queued.is_empty() {
            let snapshot = db::complete_sync_job_if_finished(&pool)
                .await
                .map_err(|error| format!("Could not complete sync job: {}", error))?
                .unwrap_or(
                    db::get_sync_snapshot(&pool)
                        .await
                        .map_err(|error| error.to_string())?,
                );
            let _ = app.emit("sync-progress-snapshot", &snapshot);
            crate::app::tray::update_sync_action(&app, &snapshot);
            transfer_stats.sync_queue(
                snapshot.total_bytes.max(0) as u64,
                snapshot.completed_bytes.max(0) as u64,
            );
            transfer_stats.set_active(false);
            emit_transfer_stats(&app, &transfer_stats);
            if is_auto {
                let _ = app.emit("sync-idle", ());
            }
            if !is_auto || totals.uploaded > 0 || totals.failed > 0 {
                let locale = app
                    .state::<LocaleState>()
                    .0
                    .lock()
                    .map(|locale| locale.clone())
                    .unwrap_or_else(|_| "en".to_string());
                let translations = backend_translations(&locale);
                let title = translations.notification_complete_title.replacen(
                    "{}",
                    if is_auto { "Auto-sync" } else { "Sync" },
                    1,
                );
                let body = translations
                    .notification_complete_body
                    .replace("{processed}", &totals.processed.to_string())
                    .replace("{uploaded}", &totals.uploaded.to_string())
                    .replace("{failed}", &totals.failed.to_string());
                let mut params = std::collections::HashMap::new();
                params.insert("processed".to_string(), totals.processed.to_string());
                params.insert("uploaded".to_string(), totals.uploaded.to_string());
                params.insert("failed".to_string(), totals.failed.to_string());
                send_notification_keyed(
                    &app,
                    &title,
                    &body,
                    "INFO",
                    "notification_complete_body",
                    params,
                );
            }
            audit_event(&app, audit_context.event("sync.completed", if totals.failed == 0 { audit::Outcome::Success } else { audit::Outcome::Failure }, if totals.failed == 0 { audit::Severity::Info } else { audit::Severity::Warn }, "Synchronization completed", json!({ "processed": totals.processed, "uploaded": totals.uploaded, "failed": totals.failed })));
            return Ok(totals);
        }
        let folders = db::get_folders(&pool)
            .await
            .map_err(|error| format!("Could not load watched folders: {error}"))?;
        let excluded_paths = excluded_paths_at_queue_boundary(&queued, &folders);
        if !excluded_paths.is_empty() {
            let snapshot = db::discard_pending_sync_paths(&pool, &excluded_paths)
                .await
                .map_err(|error| format!("Could not discard excluded queue entries: {error}"))?;
            crate::app::tray::update_sync_action(&app, &snapshot);
            progress.sync_from_snapshot(
                snapshot.total_bytes.max(0) as u64,
                snapshot.completed_bytes.max(0) as u64,
            );
            emit_transfer_stats(&app, &transfer_stats);
            let _ = app.emit("sync-progress-snapshot", &snapshot);
            continue;
        }
        let block = queued
            .into_iter()
            .map(db::QueuedUploadUnit::into_upload_unit)
            .collect::<Vec<_>>();
        let active_work = coordinator
            .register_active_work(
                block
                    .iter()
                    .flat_map(|unit| unit.assets())
                    .map(|asset| asset.path.clone()),
            )
            .await;
        let result = processor.process(block).await;
        coordinator.unregister_active_work(&active_work).await;
        let result = result?;
        totals.processed += result.processed;
        totals.uploaded += result.uploaded;
        totals.failed += result.failed;
        if let Some(snapshot) = persist_processor_results(&pool, &result).await? {
            crate::app::tray::update_sync_action(&app, &snapshot);
            progress.sync_from_snapshot(
                snapshot.total_bytes.max(0) as u64,
                snapshot.completed_bytes.max(0) as u64,
            );
            emit_transfer_stats(&app, &transfer_stats);
            let _ = app.emit("sync-progress-snapshot", &snapshot);
        }
    }
}

fn emit_transfer_stats(
    app: &tauri::AppHandle,
    transfer_stats: &crate::sync::progress::TransferStatsTracker,
) {
    let event = transfer_stats.event();
    crate::app::tray::update_transfer_status(app, event.clone());
    let _ = app.emit("sync-transfer-stats", event);
}

fn spawn_transfer_reporter(
    app: tauri::AppHandle,
    transfer_stats: crate::sync::progress::TransferStatsTracker,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(crate::sync::progress::TRANSFER_STATS_THROTTLE);
        loop {
            interval.tick().await;
            if transfer_stats.should_emit() {
                emit_transfer_stats(&app, &transfer_stats);
            }
            if !transfer_stats.status().active {
                break;
            }
        }
    });
}

struct TransferStatsCleanup {
    app: tauri::AppHandle,
    tracker: crate::sync::progress::TransferStatsTracker,
}

impl Drop for TransferStatsCleanup {
    fn drop(&mut self) {
        self.tracker.set_active(false);
        emit_transfer_stats(&self.app, &self.tracker);
    }
}

fn terminal_queue_results(summary: &SyncSummary) -> Option<&[(String, String)]> {
    (!summary.queue_results.is_empty()).then_some(&summary.queue_results)
}

/// Commits only the terminal outcomes returned by a processor block.
///
/// A processor may stop at a pause boundary after handling part of a loaded
/// block. Leaving the remainder pending is what makes a later resume lossless.
async fn persist_processor_results(
    pool: &sqlx::SqlitePool,
    summary: &SyncSummary,
) -> Result<Option<db::SyncSnapshot>, String> {
    let Some(queue_results) = terminal_queue_results(summary) else {
        return Ok(None);
    };
    db::finalize_queued_block(pool, queue_results)
        .await
        .map(Some)
        .map_err(|error| format!("Could not persist queue block: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> sqlx::SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[test]
    fn queue_window_is_limited_to_fifty_items() {
        assert_eq!(SYNC_QUEUE_BLOCK_SIZE, 50);
    }

    #[test]
    fn partial_window_only_finalizes_reported_terminal_results() {
        let summary = SyncSummary {
            processed: 2,
            uploaded: 1,
            failed: 1,
            queue_results: vec![
                ("one.jpg".to_string(), "SYNCED".to_string()),
                ("two.jpg".to_string(), "FAILED".to_string()),
            ],
        };

        assert_eq!(
            terminal_queue_results(&summary),
            Some(
                [
                    ("one.jpg".to_string(), "SYNCED".to_string()),
                    ("two.jpg".to_string(), "FAILED".to_string()),
                ]
                .as_slice()
            )
        );
    }

    #[tokio::test]
    async fn processor_outcomes_finalize_only_completed_work_and_leave_pause_remainder_pending() {
        let pool = test_pool().await;
        let units = ["first.jpg", "second.jpg", "third.jpg"]
            .into_iter()
            .map(|path| db::QueueUploadUnit {
                image: db::QueueUploadAsset {
                    path: path.to_string(),
                    size: 1,
                    mtime: 1,
                    target_album_id: None,
                },
                live_photo_video: None,
            })
            .collect::<Vec<_>>();
        db::enqueue_upload_units(&pool, &units, &[]).await.unwrap();

        let snapshot = persist_processor_results(
            &pool,
            &SyncSummary {
                processed: 2,
                uploaded: 1,
                failed: 1,
                queue_results: vec![
                    ("first.jpg".to_string(), "SYNCED".to_string()),
                    ("second.jpg".to_string(), "FAILED".to_string()),
                ],
            },
        )
        .await
        .unwrap()
        .expect("terminal processor outcomes update the durable queue");

        assert_eq!((snapshot.succeeded, snapshot.failed), (1, 1));
        let remaining = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].image.local_path, "third.jpg");
    }

    #[tokio::test]
    async fn queue_boundary_discards_pending_upload_units_matching_current_exclusions() {
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("lymic-queue-test-{}", uuid::Uuid::new_v4()));
        let excluded = root.join("Screenshots/private.jpg");
        let included = root.join("Trips/keep.jpg");
        let folders = vec![db::WatchedFolder {
            id: 1,
            path: root.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec!["screenshots/**".to_string()],
        }];
        let units = [excluded, included]
            .into_iter()
            .map(|path| db::QueueUploadUnit {
                image: db::QueueUploadAsset {
                    path: path.to_string_lossy().to_string(),
                    size: 1,
                    mtime: 1,
                    target_album_id: None,
                },
                live_photo_video: None,
            })
            .collect::<Vec<_>>();
        db::enqueue_upload_units(&pool, &units, &[]).await.unwrap();

        let queued = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        let excluded_paths = excluded_paths_at_queue_boundary(&queued, &folders);
        db::discard_pending_sync_paths(&pool, &excluded_paths)
            .await
            .unwrap();

        let remaining = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].image.local_path.ends_with("Trips/keep.jpg"));
        let snapshot = db::get_sync_snapshot(&pool).await.unwrap();
        assert_eq!(
            (snapshot.total, snapshot.succeeded, snapshot.failed),
            (1, 0, 0)
        );
    }
}
