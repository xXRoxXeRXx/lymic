//! Run-state reporting seam for Synchronization.

use crate::{
    app::{
        events::{emit_sync_error, log_to_ui_key, send_notification_keyed},
        locale::{backend_translations, LocaleState},
        tray,
    },
    audit::{self, audit_event, audit_safe_error, AuditEvent, SyncAuditContext},
    db,
    sync::{
        processor::SyncSummary,
        progress::{TransferStatsTracker, TRANSFER_STATS_THROTTLE},
        CurrentFileProgress, JobByteProgress, SyncSource,
    },
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

/// Owns side effects emitted when a Synchronization run transitions state.
pub(crate) trait SyncRunReporter {
    fn queued_waiting(&self);
    fn started(&self, snapshot: &db::SyncSnapshot);
    fn paused(&self, snapshot: &db::SyncSnapshot);
    fn resumed(&self, snapshot: &db::SyncSnapshot);
    fn progressed(&self, snapshot: &db::SyncSnapshot);
    fn completed(&self, snapshot: &db::SyncSnapshot, totals: &SyncSummary);
}

fn snapshot_byte_totals(snapshot: &db::SyncSnapshot) -> (u64, u64) {
    (
        snapshot.total_bytes.max(0) as u64,
        snapshot.completed_bytes.max(0) as u64,
    )
}

/// Stateless desktop reporter for service-level tray, event, error, and audit
/// updates outside an active run loop.
pub(crate) struct DesktopServiceReporter<R: tauri::Runtime = tauri::Wry> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> DesktopServiceReporter<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }

    pub(crate) fn claimed(&self, snapshot: &db::SyncSnapshot) {
        tray::update_sync_action(&self.app, snapshot);
    }

    pub(crate) fn progressed(&self, snapshot: &db::SyncSnapshot) {
        tray::update_sync_action(&self.app, snapshot);
        let _ = self.app.emit("sync-progress-snapshot", snapshot);
    }

    pub(crate) fn paused(&self, snapshot: &db::SyncSnapshot) {
        tray::update_sync_action(&self.app, snapshot);
        let _ = self.app.emit("sync-paused", snapshot);
    }

    pub(crate) fn resumed(&self, snapshot: &db::SyncSnapshot) {
        tray::update_sync_action(&self.app, snapshot);
        let _ = self.app.emit("sync-resumed", snapshot);
    }

    pub(crate) fn resumed_audit(&self, audit_context: &SyncAuditContext) {
        audit_event(
            &self.app,
            audit_context.event(
                "sync.resumed",
                audit::Outcome::Info,
                audit::Severity::Info,
                "Synchronization resumed",
                json!({}),
            ),
        );
    }

    pub(crate) fn error(&self, message: &str) {
        emit_sync_error(&self.app, message);
    }

    pub(crate) fn failed(
        &self,
        source: SyncSource,
        snapshot: Option<&db::SyncSnapshot>,
        error: &str,
    ) {
        if let Some(snapshot) = snapshot {
            tray::update_sync_action(&self.app, snapshot);
            let _ = self.app.emit("sync-progress-snapshot", snapshot);
        }
        audit_event(&self.app, sync_aborted_audit_event(source, error));
    }
}

struct DesktopRunTelemetry<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    source: SyncSource,
    audit_context: SyncAuditContext,
    progress: JobByteProgress,
    current_file_progress: CurrentFileProgress,
    transfer_stats: TransferStatsTracker,
    cleanup_armed: AtomicBool,
}

impl<R: tauri::Runtime> Drop for DesktopRunTelemetry<R> {
    fn drop(&mut self) {
        if self.cleanup_armed.swap(false, Ordering::SeqCst) {
            self.transfer_stats.set_active(false);
            emit_transfer_stats(&self.app, &self.transfer_stats);
        }
    }
}

/// Stateful per-run desktop adapter that publishes tray updates, Tauri events,
/// transfer statistics, completion notifications, and Synchronization audit records.
pub(crate) struct DesktopRunReporter<R: tauri::Runtime = tauri::Wry> {
    service: DesktopServiceReporter<R>,
    run: DesktopRunTelemetry<R>,
}

impl<R: tauri::Runtime> DesktopRunReporter<R> {
    pub(crate) fn new(
        app: tauri::AppHandle<R>,
        source: SyncSource,
        audit_context: SyncAuditContext,
        progress: JobByteProgress,
        current_file_progress: CurrentFileProgress,
    ) -> Self {
        let transfer_stats = progress.transfer_stats();
        Self {
            service: DesktopServiceReporter::new(app.clone()),
            run: DesktopRunTelemetry {
                app,
                source,
                audit_context,
                progress,
                current_file_progress,
                transfer_stats,
                cleanup_armed: AtomicBool::new(false),
            },
        }
    }
}

impl<R: tauri::Runtime> SyncRunReporter for DesktopRunReporter<R> {
    fn queued_waiting(&self) {
        log_to_ui_key(
            &self.run.app,
            "INFO",
            "Sync queued, waiting for running synchronization.",
            "log_sync_queued_waiting",
        );
    }

    fn started(&self, snapshot: &db::SyncSnapshot) {
        let _ = self.run.app.emit("sync-started", ());
        let _ = self.run.app.emit("sync-progress-snapshot", snapshot);
        audit_event(
            &self.run.app,
            self.run.audit_context.event(
                "sync.started",
                audit::Outcome::Started,
                audit::Severity::Info,
                "Synchronization started",
                json!({ "source": self.run.source.audit_label() }),
            ),
        );
        let (total_bytes, completed_bytes) = snapshot_byte_totals(snapshot);
        self.run
            .progress
            .sync_from_snapshot(total_bytes, completed_bytes);
        self.run.transfer_stats.set_active(true);
        self.run.cleanup_armed.store(true, Ordering::SeqCst);
        spawn_transfer_reporter(self.run.app.clone(), self.run.transfer_stats.clone());
        emit_transfer_stats(&self.run.app, &self.run.transfer_stats);
    }

    fn paused(&self, snapshot: &db::SyncSnapshot) {
        let (total_bytes, completed_bytes) = snapshot_byte_totals(snapshot);
        self.run
            .transfer_stats
            .sync_queue(total_bytes, completed_bytes);
        self.run.transfer_stats.set_paused(true);
        self.run.current_file_progress.emit(&self.run.app, "");
        emit_transfer_stats(&self.run.app, &self.run.transfer_stats);
        self.service.paused(snapshot);
        audit_event(
            &self.run.app,
            self.run.audit_context.event(
                "sync.paused",
                audit::Outcome::Info,
                audit::Severity::Info,
                "Synchronization paused",
                json!({}),
            ),
        );
    }

    fn resumed(&self, snapshot: &db::SyncSnapshot) {
        let (total_bytes, completed_bytes) = snapshot_byte_totals(snapshot);
        self.run
            .transfer_stats
            .sync_queue(total_bytes, completed_bytes);
        self.run.transfer_stats.set_paused(false);
        emit_transfer_stats(&self.run.app, &self.run.transfer_stats);
        self.service.resumed(snapshot);
        self.service.resumed_audit(&self.run.audit_context);
    }

    fn progressed(&self, snapshot: &db::SyncSnapshot) {
        let (total_bytes, completed_bytes) = snapshot_byte_totals(snapshot);
        self.run
            .progress
            .sync_from_snapshot(total_bytes, completed_bytes);
        emit_transfer_stats(&self.run.app, &self.run.transfer_stats);
        self.service.progressed(snapshot);
    }

    fn completed(&self, snapshot: &db::SyncSnapshot, totals: &SyncSummary) {
        self.service.progressed(snapshot);
        let (total_bytes, completed_bytes) = snapshot_byte_totals(snapshot);
        self.run
            .transfer_stats
            .sync_queue(total_bytes, completed_bytes);
        self.run.transfer_stats.set_active(false);
        self.run.cleanup_armed.store(false, Ordering::SeqCst);
        self.run.current_file_progress.emit(&self.run.app, "");
        emit_transfer_stats(&self.run.app, &self.run.transfer_stats);
        let is_automatic = !self.run.source.is_manual();
        if is_automatic {
            let _ = self.run.app.emit("sync-idle", ());
        }
        if !is_automatic || totals.uploaded > 0 || totals.failed > 0 {
            let locale = self
                .run
                .app
                .try_state::<LocaleState>()
                .and_then(|state| state.0.lock().ok().map(|locale| locale.clone()))
                .unwrap_or_else(|| "en".to_string());
            let translations = backend_translations(&locale);
            let title = translations.notification_complete_title.replacen(
                "{}",
                if is_automatic { "Auto-sync" } else { "Sync" },
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
                &self.run.app,
                &title,
                &body,
                "INFO",
                "notification_complete_body",
                params,
            );
        }
        audit_event(
            &self.run.app,
            self.run.audit_context.event(
                "sync.completed",
                if totals.failed == 0 {
                    audit::Outcome::Success
                } else {
                    audit::Outcome::Failure
                },
                if totals.failed == 0 {
                    audit::Severity::Info
                } else {
                    audit::Severity::Warn
                },
                "Synchronization completed",
                json!({
                    "processed": totals.processed,
                    "uploaded": totals.uploaded,
                    "failed": totals.failed,
                }),
            ),
        );
    }
}

fn emit_transfer_stats<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    transfer_stats: &TransferStatsTracker,
) {
    let event = transfer_stats.event();
    tray::update_transfer_status(app, event.clone());
    let _ = app.emit("sync-transfer-stats", event);
}

fn spawn_transfer_reporter<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    transfer_stats: TransferStatsTracker,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TRANSFER_STATS_THROTTLE);
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

pub(crate) fn sync_aborted_audit_event(source: SyncSource, error: &str) -> AuditEvent {
    AuditEvent::new(
        AuditEvent::operation_id(),
        "sync.aborted",
        audit::Outcome::Failure,
        audit::Severity::Error,
        None,
        source.audit_label(),
        "Synchronization aborted",
        json!({ "failure_reason": audit_safe_error(error, &[]) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audit::{AuditLogger, SyncAuditContext},
        auth::AuthConfig,
        sync::{CurrentFileProgress, JobByteProgress},
    };
    use std::sync::{Arc, Mutex};
    use tauri::{Listener, Manager};

    fn test_snapshot(
        status: db::SyncJobStatus,
        total: i64,
        succeeded: i64,
        failed: i64,
    ) -> db::SyncSnapshot {
        db::SyncSnapshot {
            status,
            total,
            succeeded,
            failed,
            total_bytes: total * 100,
            completed_bytes: (succeeded + failed) * 100,
            current_path: None,
        }
    }

    fn test_audit_context(source: SyncSource) -> SyncAuditContext {
        let credentials = AuthConfig {
            server_url: "https://immich.example".to_string(),
            api_key: "secret-key".to_string(),
            cloudflare_client_id: None,
            cloudflare_client_secret: None,
            allow_local_http: false,
        };
        SyncAuditContext::from_credentials(&credentials, source.audit_label())
    }

    #[tokio::test]
    async fn desktop_reporter_emits_lifecycle_events_and_audit_records() {
        let app = tauri::test::mock_app();
        let audit_logger = Arc::new(AuditLogger::initialize(app.handle()).unwrap());
        app.manage(audit_logger.clone());
        app.manage(crate::app::state::TraySyncStatus(Mutex::new(
            db::SyncJobStatus::Idle,
        )));
        app.manage(crate::app::state::TrayTransferStatus(Mutex::new(None)));

        let events = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
        for name in [
            "sync-started",
            "sync-progress-snapshot",
            "sync-paused",
            "sync-resumed",
            "sync-transfer-stats",
            "sync-progress",
            "sync-idle",
            "sync-error",
        ] {
            let captured = events.clone();
            let event_name = name.to_string();
            app.handle().listen(name, move |event| {
                let payload = serde_json::from_str::<serde_json::Value>(event.payload())
                    .unwrap_or_else(|_| serde_json::Value::String(event.payload().to_string()));
                captured.lock().unwrap().push((event_name.clone(), payload));
            });
        }

        let progress = JobByteProgress::new(0, 0);
        let current_file = CurrentFileProgress::default();
        let audit_context = test_audit_context(SyncSource::Manual);
        let expected_op_id = serde_json::to_value(audit_context.event(
            "probe",
            audit::Outcome::Info,
            audit::Severity::Info,
            "probe",
            json!({}),
        ))
        .unwrap()["operation_id"]
            .as_str()
            .unwrap()
            .to_string();
        let reporter = DesktopRunReporter::new(
            app.handle().clone(),
            SyncSource::Manual,
            audit_context,
            progress,
            current_file,
        );

        let started = test_snapshot(db::SyncJobStatus::Running, 2, 0, 0);
        reporter.started(&started);
        let paused = test_snapshot(db::SyncJobStatus::Paused, 2, 1, 0);
        reporter.paused(&paused);
        let resumed = test_snapshot(db::SyncJobStatus::Running, 2, 1, 0);
        reporter.resumed(&resumed);
        let progressed = test_snapshot(db::SyncJobStatus::Running, 2, 2, 0);
        reporter.progressed(&progressed);
        let completed = test_snapshot(db::SyncJobStatus::Idle, 2, 2, 0);
        reporter.completed(
            &completed,
            &SyncSummary {
                processed: 2,
                uploaded: 2,
                failed: 0,
                queue_results: Vec::new(),
            },
        );

        let captured = events.lock().unwrap();
        let names = captured
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"sync-started"));
        assert!(names.contains(&"sync-paused"));
        assert!(names.contains(&"sync-resumed"));
        assert!(names.contains(&"sync-progress-snapshot"));
        assert!(names.contains(&"sync-transfer-stats"));
        assert!(names.contains(&"sync-progress"));
        assert!(!names.contains(&"sync-idle"));

        let tray_sync = *app
            .handle()
            .state::<crate::app::state::TraySyncStatus>()
            .0
            .lock()
            .unwrap();
        assert_eq!(tray_sync, db::SyncJobStatus::Idle);

        audit_logger.flush().unwrap();
        let audit_log = std::fs::read_to_string(
            app.handle()
                .path()
                .app_data_dir()
                .unwrap()
                .join("logs")
                .join("audit-0.jsonl"),
        )
        .unwrap();
        let event_types = audit_log
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["operation_id"] == expected_op_id)
            .map(|event| event["event_type"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                "sync.started",
                "sync.paused",
                "sync.resumed",
                "sync.completed"
            ]
        );
    }

    #[tokio::test]
    async fn desktop_service_reporter_updates_tray_and_emits_service_events() {
        let app = tauri::test::mock_app();
        let audit_logger = Arc::new(AuditLogger::initialize(app.handle()).unwrap());
        app.manage(audit_logger.clone());
        app.manage(crate::app::state::TraySyncStatus(Mutex::new(
            db::SyncJobStatus::Idle,
        )));

        let events = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
        for name in [
            "sync-progress-snapshot",
            "sync-paused",
            "sync-resumed",
            "sync-error",
        ] {
            let captured = events.clone();
            let event_name = name.to_string();
            app.handle().listen(name, move |event| {
                let payload = serde_json::from_str::<serde_json::Value>(event.payload())
                    .unwrap_or_else(|_| serde_json::Value::String(event.payload().to_string()));
                captured.lock().unwrap().push((event_name.clone(), payload));
            });
        }

        let reporter = DesktopServiceReporter::new(app.handle().clone());
        let running = test_snapshot(db::SyncJobStatus::Running, 2, 0, 0);
        let paused = test_snapshot(db::SyncJobStatus::Paused, 2, 1, 0);
        let failed = test_snapshot(db::SyncJobStatus::Failed, 2, 1, 1);

        reporter.claimed(&running);
        assert_eq!(
            *app.handle()
                .state::<crate::app::state::TraySyncStatus>()
                .0
                .lock()
                .unwrap(),
            db::SyncJobStatus::Running
        );

        reporter.paused(&paused);
        reporter.resumed(&running);
        let audit_context = test_audit_context(SyncSource::Recovery);
        let expected_op_id = serde_json::to_value(audit_context.event(
            "probe",
            audit::Outcome::Info,
            audit::Severity::Info,
            "probe",
            json!({}),
        ))
        .unwrap()["operation_id"]
            .as_str()
            .unwrap()
            .to_string();
        reporter.resumed_audit(&audit_context);
        reporter.failed(SyncSource::Watcher, Some(&failed), "connection lost");
        reporter.error("connection lost");

        assert_eq!(
            *app.handle()
                .state::<crate::app::state::TraySyncStatus>()
                .0
                .lock()
                .unwrap(),
            db::SyncJobStatus::Failed
        );

        let captured = events.lock().unwrap();
        assert_eq!(
            captured
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "sync-paused",
                "sync-resumed",
                "sync-progress-snapshot",
                "sync-error",
            ]
        );
        assert_eq!(captured[0].1["status"], "PAUSED");
        assert_eq!(captured[1].1["status"], "RUNNING");
        assert_eq!(captured[2].1["status"], "FAILED");
        assert_eq!(captured[3].1, serde_json::json!("connection lost"));

        audit_logger.flush().unwrap();
        let audit_log = std::fs::read_to_string(
            app.handle()
                .path()
                .app_data_dir()
                .unwrap()
                .join("logs")
                .join("audit-0.jsonl"),
        )
        .unwrap();
        let resumed = audit_log
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["operation_id"] == expected_op_id)
            .map(|event| event["event_type"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(resumed, vec!["sync.resumed"]);
    }

    #[tokio::test]
    async fn desktop_reporter_applies_completion_notification_rules_for_manual_and_automatic_runs()
    {
        async fn run_completion(
            source: SyncSource,
            totals: SyncSummary,
        ) -> (Vec<String>, Vec<serde_json::Value>) {
            let app = tauri::test::mock_app();
            let audit_logger = Arc::new(AuditLogger::initialize(app.handle()).unwrap());
            app.manage(audit_logger);
            app.manage(crate::app::locale::LocaleState(Mutex::new(
                "en".to_string(),
            )));
            let logs = crate::app::events::CapturedUiLogs::listen(app.handle());
            let idle_events = Arc::new(Mutex::new(Vec::new()));
            let captured_idle = idle_events.clone();
            app.handle().listen("sync-idle", move |_| {
                captured_idle.lock().unwrap().push("sync-idle".to_string());
            });

            let reporter = DesktopRunReporter::new(
                app.handle().clone(),
                source,
                test_audit_context(source),
                JobByteProgress::new(0, 0),
                CurrentFileProgress::default(),
            );
            let snapshot = test_snapshot(db::SyncJobStatus::Idle, 1, 0, totals.failed as i64);
            reporter.completed(&snapshot, &totals);
            let idle = idle_events.lock().unwrap().clone();
            (idle, logs.with_key("notification_complete_body"))
        }

        // Manual run with zero uploads/failures still notifies and does not emit sync-idle.
        let (idle, notifications) = run_completion(
            SyncSource::Manual,
            SyncSummary {
                processed: 1,
                uploaded: 0,
                failed: 0,
                queue_results: Vec::new(),
            },
        )
        .await;
        assert!(idle.is_empty());
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["params"]["processed"], "1");
        assert_eq!(notifications[0]["params"]["uploaded"], "0");
        assert_eq!(notifications[0]["params"]["failed"], "0");

        // Automatic run with zero uploads/failures emits sync-idle and suppresses notification.
        let (idle, notifications) = run_completion(
            SyncSource::Watcher,
            SyncSummary {
                processed: 1,
                uploaded: 0,
                failed: 0,
                queue_results: Vec::new(),
            },
        )
        .await;
        assert_eq!(idle, vec!["sync-idle"]);
        assert!(notifications.is_empty());

        // Automatic run with uploaded > 0 emits sync-idle and sends notification.
        let (idle, notifications) = run_completion(
            SyncSource::Watcher,
            SyncSummary {
                processed: 1,
                uploaded: 1,
                failed: 0,
                queue_results: Vec::new(),
            },
        )
        .await;
        assert_eq!(idle, vec!["sync-idle"]);
        assert_eq!(notifications.len(), 1);

        // Automatic run with failed > 0 emits sync-idle and sends notification.
        let (idle, notifications) = run_completion(
            SyncSource::Startup,
            SyncSummary {
                processed: 1,
                uploaded: 0,
                failed: 1,
                queue_results: Vec::new(),
            },
        )
        .await;
        assert_eq!(idle, vec!["sync-idle"]);
        assert_eq!(notifications.len(), 1);
    }
}
