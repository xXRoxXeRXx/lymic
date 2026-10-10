//! Durable synchronization queue execution.

use crate::{
    app::{
        events::send_notification,
        locale::{backend_translations, LocaleState},
    },
    audit::{self, audit_event, SyncAuditContext},
    db,
    immich::ImmichClient,
    media::model::{Asset, ScanResult},
    sync::{
        processor::{process_sync_block, SyncSummary},
        JobByteProgress, SyncCoordinatorInner,
    },
};
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager};

const SYNC_QUEUE_BLOCK_SIZE: i64 = 50;

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
    let queue_assets: Vec<_> = scan_result
        .files
        .iter()
        .map(|asset| db::QueueAsset {
            path: &asset.path,
            size: asset.size as i64,
            mtime: asset.mtime,
        })
        .collect();
    let queue_failures: Vec<_> = scan_result
        .failures
        .iter()
        .map(|failure| (failure.path.clone(), failure.error.clone()))
        .collect();
    let snapshot = db::enqueue_sync_assets(&pool, &queue_assets, &queue_failures)
        .await
        .map_err(|error| format!("Could not persist sync queue: {}", error))?;
    crate::app::tray::update_sync_action(&app, &snapshot);
    let _sync_guard = match coordinator.lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if !is_auto {
                crate::app::events::log_to_ui(
                    &app,
                    "INFO",
                    "Sync queued, waiting for running synchronization.",
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
        let queued = db::next_sync_queue_block(&pool, SYNC_QUEUE_BLOCK_SIZE)
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
                send_notification(&app, &title, &body, "INFO");
            }
            audit_event(&app, audit_context.event("sync.completed", if totals.failed == 0 { audit::Outcome::Success } else { audit::Outcome::Failure }, if totals.failed == 0 { audit::Severity::Info } else { audit::Severity::Warn }, "Synchronization completed", json!({ "processed": totals.processed, "uploaded": totals.uploaded, "failed": totals.failed })));
            return Ok(totals);
        }
        let block = ScanResult {
            files: queued
                .into_iter()
                .map(|asset| Asset {
                    path: asset.local_path,
                    size: asset.size as u64,
                    mtime: asset.last_modified,
                })
                .collect(),
            ..Default::default()
        };
        let active_work = coordinator
            .register_active_work(block.files.iter().map(|asset| asset.path.clone()))
            .await;
        let result = process_sync_block(
            app.clone(),
            pool.clone(),
            client.clone(),
            block,
            audit_context.clone(),
            progress.clone(),
            coordinator.clone(),
        )
        .await;
        coordinator.unregister_active_work(&active_work).await;
        let result = result?;
        totals.processed += result.processed;
        totals.uploaded += result.uploaded;
        totals.failed += result.failed;
        if let Some(queue_results) = terminal_queue_results(&result) {
            let snapshot = db::finalize_queued_block(&pool, queue_results)
                .await
                .map_err(|error| format!("Could not persist queue block: {}", error))?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
