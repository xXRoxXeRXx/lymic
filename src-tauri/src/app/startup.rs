use crate::app::events::{log_to_ui_folder, log_to_ui_folder_error, send_notification_keyed};
use crate::app::state::{DatabaseRecoveryNotice, DatabaseRecoveryNoticeData};
use crate::{
    audit::{self, audit_event, audit_safe_error},
    db, sync, watcher,
};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use tauri::Manager;

pub(crate) const WATCH_EVENT_QUEUE_CAPACITY: usize = 4_096;

pub(crate) struct StartupContext {
    pub(crate) has_folders: bool,
    pub(crate) recovered_sync: db::SyncSnapshot,
    pub(crate) watch_events: tokio::sync::mpsc::Receiver<notify::Event>,
    pub(crate) rescan_requested: std::sync::Arc<AtomicBool>,
    pub(crate) rescan_notify: std::sync::Arc<tokio::sync::Notify>,
    pub(crate) volume_events: tokio::sync::mpsc::Receiver<crate::removable::VolumeEvent>,
}

pub(crate) fn initialize(app: &tauri::App) -> Result<StartupContext, Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let audit_logger = audit::AuditLogger::initialize(&handle)
        .map_err(|error| format!("Failed to initialize audit logger: {error}"))?;
    handle.manage(std::sync::Arc::new(audit_logger));

    record_startup_phase(&handle, "audit_logger_ready", audit::Outcome::Success, None);

    let (tx, watch_events) = tokio::sync::mpsc::channel(WATCH_EVENT_QUEUE_CAPACITY);
    let rescan_requested = std::sync::Arc::new(AtomicBool::new(false));
    let rescan_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    record_startup_phase(
        &handle,
        "watcher_initializing",
        audit::Outcome::Started,
        None,
    );
    let watcher = watcher::create_watcher(tx, rescan_requested.clone(), rescan_notify.clone())
        .map_err(|error| {
            record_startup_phase(
                &handle,
                "watcher_initialization_failed",
                audit::Outcome::Failure,
                Some(error.to_string()),
            );
            error.to_string()
        })?;
    record_startup_phase(&handle, "watcher_ready", audit::Outcome::Success, None);

    // If DB initialization fails completely, setup exits cleanly. Corruption is repaired by db::init.
    record_startup_phase(
        &handle,
        "database_initializing",
        audit::Outcome::Started,
        None,
    );
    let db_init = tauri::async_runtime::block_on(db::init(&handle)).map_err(|error| {
        let message = format!("Failed to initialize database: {error}");
        record_startup_phase(
            &handle,
            "database_initialization_failed",
            audit::Outcome::Failure,
            Some(message.clone()),
        );
        message
    })?;
    let pool = db_init.pool;
    record_startup_phase(&handle, "database_ready", audit::Outcome::Success, None);
    record_startup_phase(
        &handle,
        "sync_recovery_starting",
        audit::Outcome::Started,
        None,
    );
    let recovered_sync =
        tauri::async_runtime::block_on(db::recover_sync_job(&pool)).map_err(|error| {
            let message = format!("Failed to recover sync queue: {error}");
            record_startup_phase(
                &handle,
                "sync_recovery_failed",
                audit::Outcome::Failure,
                Some(message.clone()),
            );
            message
        })?;
    record_startup_phase(
        &handle,
        "sync_recovery_complete",
        audit::Outcome::Success,
        None,
    );

    let mut recovery_notice = None;
    if let db::DatabaseStatus::Repaired {
        ref backup_path,
        salvaged_folders,
        ref reason,
    } = db_init.status
    {
        let backup_str = backup_path.display().to_string();
        let notice = format!(
            "Database could not be used ({}). It was backed up to '{}' and rebuilt; restored {} watched folder(s). Other stored app data, including settings, sync queue history, and hash cache, could not be restored.",
            reason,
            backup_str,
            salvaged_folders
        );
        let system_locale = crate::app::locale::detect_system_locale();
        let translations = crate::app::locale::backend_translations(system_locale);
        let title = translations
            .notification_database_repaired_title
            .as_deref()
            .unwrap_or("Lymic - Database Repaired");
        let body = translations
            .log_database_repaired_notice
            .as_deref()
            .map(|template| {
                template
                    .replace("{reason}", reason)
                    .replace("{backup}", &backup_str)
                    .replace("{count}", &salvaged_folders.to_string())
            })
            .unwrap_or_else(|| notice.clone());

        let mut params = std::collections::HashMap::new();
        params.insert("reason".to_string(), reason.clone());
        params.insert("backup".to_string(), backup_str.clone());
        params.insert("count".to_string(), salvaged_folders.to_string());
        send_notification_keyed(
            &handle,
            title,
            &body,
            "WARN",
            "log_database_repaired_notice",
            params,
        );
        audit_event(
            &handle,
            audit::AuditEvent::new(
                audit::AuditEvent::operation_id(),
                "database.repaired",
                audit::Outcome::Success,
                audit::Severity::Warn,
                None,
                "startup",
                "Database backed up and rebuilt",
                json!({ "backup_path": backup_path.to_string_lossy(), "salvaged_folders": salvaged_folders, "reason": reason }),
            ),
        );
        recovery_notice = Some(DatabaseRecoveryNoticeData {
            message: notice,
            key: "log_database_repaired_notice".to_string(),
            reason: reason.clone(),
            backup: backup_str,
            count: salvaged_folders,
        });
    }

    let mut watcher = watcher::WatcherState::new(watcher);
    let folders = tauri::async_runtime::block_on(db::get_folders(&pool))
        .map_err(|error| format!("Failed to load watched folders: {error}"))?;
    let has_folders = !folders.is_empty();
    for folder in folders {
        match watcher::watch_path(&mut watcher, &folder.path) {
            Ok(watcher::WatchRegistration::PathUnavailable) => {
                log_to_ui_folder(
                    &handle,
                    "WARN",
                    &format!(
                        "Folder '{}' offline at startup; watching deferred.",
                        folder.path
                    ),
                    "log_folder_offline_startup",
                    &folder.path,
                );
                audit_event(
                    &handle,
                    audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "watcher.registration",
                        audit::Outcome::Skipped,
                        audit::Severity::Warn,
                        None,
                        "startup",
                        "Watched folder is unavailable",
                        json!({ "local_path": folder.path }),
                    ),
                );
            }
            Err(error) => {
                log_to_ui_folder_error(
                    &handle,
                    "WARN",
                    &format!("Could not watch '{}': {}", folder.path, error),
                    "log_could_not_watch_folder",
                    &folder.path,
                    &error.to_string(),
                );
                audit_event(
                    &handle,
                    audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "watcher.registration",
                        audit::Outcome::Failure,
                        audit::Severity::Warn,
                        None,
                        "startup",
                        "Watcher registration failed",
                        json!({ "local_path": folder.path, "failure_reason": audit_safe_error(&error.to_string(), &[]) }),
                    ),
                );
            }
            Ok(_) => audit_event(
                &handle,
                audit::AuditEvent::new(
                    audit::AuditEvent::operation_id(),
                    "watcher.registration",
                    audit::Outcome::Success,
                    audit::Severity::Info,
                    None,
                    "startup",
                    "Watcher registered",
                    json!({ "local_path": folder.path }),
                ),
            ),
        }
    }

    handle.manage(pool);
    handle.manage(tokio::sync::Mutex::new(watcher));
    handle.manage(sync::SyncState(AtomicBool::new(false)));
    handle.manage(sync::SyncCoordinator(std::sync::Arc::new(
        sync::SyncCoordinatorInner::new(recovered_sync.status == "PAUSED"),
    )));
    handle.manage(crate::app::locale::LocaleState(std::sync::Mutex::new(
        "en".to_string(),
    )));
    handle.manage(DatabaseRecoveryNotice(std::sync::Mutex::new(
        recovery_notice,
    )));
    handle.manage(crate::removable::ActiveRemovableVolumes::new());

    audit_event(
        &handle,
        audit::AuditEvent::new(
            audit::AuditEvent::operation_id(),
            "app.started",
            audit::Outcome::Success,
            audit::Severity::Info,
            None,
            "startup",
            "Application started",
            json!({}),
        ),
    );

    let (volume_tx, volume_events) = tokio::sync::mpsc::channel(64);
    let removable_monitor = crate::removable::create_platform_monitor();
    if let Err(error) = removable_monitor.start(volume_tx) {
        log_to_ui_folder_error(
            &handle,
            "WARN",
            &format!("Failed to start removable volume monitor: {error}"),
            "log_removable_monitor_failed",
            "",
            &error,
        );
    }

    Ok(StartupContext {
        has_folders,
        recovered_sync,
        watch_events,
        rescan_requested,
        rescan_notify,
        volume_events,
    })
}

pub(crate) fn record_startup_phase(
    app: &tauri::AppHandle,
    phase: &str,
    outcome: audit::Outcome,
    failure_reason: Option<String>,
) {
    audit_event(
        app,
        audit::AuditEvent::new(
            audit::AuditEvent::operation_id(),
            "app.startup.phase",
            outcome,
            if failure_reason.is_some() {
                audit::Severity::Error
            } else {
                audit::Severity::Info
            },
            None,
            "startup",
            phase,
            json!({ "phase": phase, "failure_reason": failure_reason }),
        ),
    );
}
