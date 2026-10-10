use crate::app::events::{log_to_ui, send_notification};
use crate::app::state::DatabaseRecoveryNotice;
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
    pub(crate) watch_events: tokio::sync::mpsc::Receiver<notify::Event>,
    pub(crate) rescan_requested: std::sync::Arc<AtomicBool>,
    pub(crate) rescan_notify: std::sync::Arc<tokio::sync::Notify>,
}

pub(crate) fn initialize(app: &tauri::App) -> Result<StartupContext, Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let audit_logger = audit::AuditLogger::initialize(&handle)
        .map_err(|error| format!("Failed to initialize audit logger: {error}"))?;
    handle.manage(std::sync::Arc::new(audit_logger));

    let (tx, watch_events) = tokio::sync::mpsc::channel(WATCH_EVENT_QUEUE_CAPACITY);
    let rescan_requested = std::sync::Arc::new(AtomicBool::new(false));
    let rescan_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let watcher = watcher::create_watcher(tx, rescan_requested.clone(), rescan_notify.clone())
        .map_err(|error| error.to_string())?;

    // If DB initialization fails completely, setup exits cleanly. Corruption is repaired by db::init.
    let db_init = tauri::async_runtime::block_on(db::init(&handle))
        .map_err(|error| format!("Failed to initialize database: {error}"))?;
    let pool = db_init.pool;
    let recovered_sync = tauri::async_runtime::block_on(db::recover_sync_job(&pool))
        .map_err(|error| format!("Failed to recover sync queue: {error}"))?;

    let mut recovery_notice = None;
    if let db::DatabaseStatus::Repaired {
        ref backup_path,
        salvaged_folders,
        ref reason,
    } = db_init.status
    {
        let notice = format!(
            "Database corruption detected ({}). Quarantined backup to '{}' and restored {} folder(s).",
            reason,
            backup_path.display(),
            salvaged_folders
        );
        send_notification(&handle, "Lymic - Database Repaired", &notice, "WARN");
        audit_event(
            &handle,
            audit::AuditEvent::new(
                audit::AuditEvent::operation_id(),
                "database.repaired",
                audit::Outcome::Success,
                audit::Severity::Warn,
                None,
                "startup",
                "Database corruption detected and repaired",
                json!({ "backup_path": backup_path.to_string_lossy(), "salvaged_folders": salvaged_folders, "reason": reason }),
            ),
        );
        recovery_notice = Some(notice);
    }

    let mut watcher = watcher::WatcherState::new(watcher);
    let folders = tauri::async_runtime::block_on(db::get_folders(&pool))
        .map_err(|error| format!("Failed to load watched folders: {error}"))?;
    let has_folders = !folders.is_empty();
    for folder in folders {
        match watcher::watch_path(&mut watcher, &folder.path) {
            Ok(watcher::WatchRegistration::PathUnavailable) => {
                log_to_ui(
                    &handle,
                    "WARN",
                    &format!(
                        "Folder '{}' offline at startup; watching deferred.",
                        folder.path
                    ),
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
                log_to_ui(
                    &handle,
                    "WARN",
                    &format!("Could not watch '{}': {}", folder.path, error),
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
                        json!({ "local_path": folder.path, "failure_reason": audit_safe_error(&error.to_string(), None, None) }),
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

    Ok(StartupContext {
        has_folders,
        watch_events,
        rescan_requested,
        rescan_notify,
    })
}
