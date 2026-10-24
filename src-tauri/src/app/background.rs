use crate::app::events::{
    log_to_ui_error, log_to_ui_folder, log_to_ui_folder_error, log_to_ui_key,
};
use crate::app::startup::StartupContext;
use crate::{
    audit::{self, audit_event},
    db,
    sync::{admit_and_run, log_overlapping_folders, scan_watched_folders_for_media, SyncSource},
    watcher::{WatcherIngestion, WATCHER_DEBOUNCE},
};
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::Manager;

pub(crate) fn spawn(handle: tauri::AppHandle, startup: StartupContext) {
    spawn_deferred_watches(handle.clone());
    spawn_startup_sync(handle.clone());
    spawn_watcher_consumer(
        handle.clone(),
        startup.watch_events,
        startup.rescan_requested,
        startup.rescan_notify,
    );
    spawn_removable_volume_consumer(handle, startup.volume_events);
}

fn spawn_removable_volume_consumer(
    handle: tauri::AppHandle,
    mut volume_events: tokio::sync::mpsc::Receiver<crate::removable::VolumeEvent>,
) {
    tauri::async_runtime::spawn(async move {
        let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
        let active_volumes = handle
            .state::<crate::removable::ActiveRemovableVolumes>()
            .inner()
            .clone();
        let coordinator = crate::removable::RemovableVolumeCoordinator::new(pool, handle.clone())
            .with_active_volumes(active_volumes);
        while let Some(event) = volume_events.recv().await {
            if let Err(error) = coordinator.handle_event(event).await {
                log_to_ui_error(
                    &handle,
                    "WARN",
                    &format!("Error handling removable volume event: {error}"),
                    "log_removable_volume_error",
                    &error,
                );
            }
        }
    });
}

fn spawn_deferred_watches(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut retry_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        retry_interval.tick().await;
        loop {
            retry_interval.tick().await;
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let folders = match db::get_folders(&pool).await {
                Ok(folders) => folders,
                Err(error) => {
                    log_to_ui_error(
                        &handle,
                        "ERROR",
                        &format!("Could not load deferred watched folders: {error}"),
                        "log_load_deferred_folders_failed",
                        &error.to_string(),
                    );
                    continue;
                }
            };
            let available_paths = match tokio::task::spawn_blocking(move || {
                folders
                    .into_iter()
                    .map(|folder| {
                        let path = std::path::PathBuf::from(folder.path);
                        let is_available = path.is_dir();
                        (path, is_available)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            {
                Ok(paths) => paths,
                Err(error) => {
                    log_to_ui_error(
                        &handle,
                        "ERROR",
                        &format!("Could not check deferred watched folders: {error}"),
                        "log_check_deferred_folders_failed",
                        &error.to_string(),
                    );
                    continue;
                }
            };
            let registered_paths = {
                let watcher = handle.state::<tokio::sync::Mutex<crate::watcher::WatcherState>>();
                let mut watcher = watcher.lock().await;
                available_paths
                    .into_iter()
                    .filter_map(|(path, is_available)| {
                        match watcher.reconcile_path(&path, is_available) {
                            Ok(crate::watcher::WatchRegistration::Registered) => Some(path),
                            Ok(_) => None,
                            Err(error) => {
                                log_to_ui_folder_error(
                                    &handle,
                                    "WARN",
                                    &format!("Could not watch '{}': {}", path.display(), error),
                                    "log_could_not_watch_folder",
                                    &path.display().to_string(),
                                    &error.to_string(),
                                );
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            };
            if registered_paths.is_empty() {
                continue;
            }
            for path in &registered_paths {
                log_to_ui_folder(
                    &handle,
                    "INFO",
                    &format!(
                        "Folder '{}' is available; watching enabled.",
                        path.display()
                    ),
                    "log_folder_available_watching_enabled",
                    &path.display().to_string(),
                );
                audit_event(
                    &handle,
                    audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "watcher.reconciliation",
                        audit::Outcome::Success,
                        audit::Severity::Info,
                        None,
                        "deferred_folder",
                        "Watcher registration reconciled",
                        json!({ "local_path": path }),
                    ),
                );
            }
            let scan_folders = match db::get_folders(&pool).await {
                Ok(folders) => folders
                    .into_iter()
                    .filter(|folder| {
                        registered_paths
                            .iter()
                            .any(|path| path == &std::path::PathBuf::from(&folder.path))
                    })
                    .collect(),
                Err(error) => {
                    log_to_ui_error(
                        &handle,
                        "ERROR",
                        &format!("Could not load watched folders for scan: {error}"),
                        "log_scan_folders_failed",
                        &error.to_string(),
                    );
                    continue;
                }
            };
            let scan_result = match scan_watched_folders_for_media(scan_folders).await {
                Ok(scan_result) => scan_result,
                Err(error) => {
                    log_to_ui_error(
                        &handle,
                        "ERROR",
                        &format!("Folder scanning failed: {error}"),
                        "log_scan_folders_failed",
                        &error,
                    );
                    continue;
                }
            };
            admit_and_run(
                handle.clone(),
                pool,
                scan_result,
                SyncSource::DeferredFolder,
            )
            .await;
        }
    });
}

fn spawn_startup_sync(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
        let folders = match db::get_folders(&pool).await {
            Ok(folders) => folders,
            Err(error) => {
                log_to_ui_error(
                    &handle,
                    "ERROR",
                    &format!("Could not load watched folders for startup sync: {error}"),
                    "log_load_folders_for_startup_sync_failed",
                    &error.to_string(),
                );
                return;
            }
        };
        for folder in &folders {
            log_to_ui_folder(
                &handle,
                "INFO",
                &format!("Scanning folder: {}", folder.path),
                "log_scanning_folder",
                &folder.path,
            );
        }
        let scan_result = match scan_watched_folders_for_media(folders).await {
            Ok(result) => result,
            Err(error) => {
                log_to_ui_error(
                    &handle,
                    "ERROR",
                    &format!("Folder scanning failed: {error}"),
                    "log_scan_folders_failed",
                    &error,
                );
                return;
            }
        };
        admit_and_run(handle.clone(), pool, scan_result, SyncSource::Startup).await;
    });
}

fn spawn_watcher_consumer(
    handle: tauri::AppHandle,
    mut watch_events: tokio::sync::mpsc::Receiver<crate::watcher::WatcherInput>,
    rescan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _rescan_notify: std::sync::Arc<tokio::sync::Notify>,
) {
    tauri::async_runtime::spawn(async move {
        let mut ingestion = WatcherIngestion::new(WATCHER_DEBOUNCE);
        loop {
            let Some(mut batch) = ingestion.next_batch(&mut watch_events).await else {
                return;
            };
            // A full native queue cannot carry an explicit Overflow input. The callback
            // records that fact atomically; the ingestion boundary consumes it here.
            if rescan_requested.swap(false, Ordering::AcqRel) {
                batch.requires_full_rescan = true;
            }
            if batch.requires_full_rescan {
                log_to_ui_key(
                    &handle,
                    "WARN",
                    "File watcher event queue overflowed; reconciling all watched folders.",
                    "log_watcher_queue_overflow",
                );
                audit_event(
                    &handle,
                    audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "watcher.rescan",
                        audit::Outcome::Started,
                        audit::Severity::Warn,
                        None,
                        "watcher",
                        "Watcher queue overflow triggered rescan",
                        json!({}),
                    ),
                );
            }
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let folders = match db::get_folders(&pool).await {
                Ok(folders) => folders,
                Err(error) => {
                    log_to_ui_error(
                        &handle,
                        "ERROR",
                        &format!("Could not load watched folders for watcher event: {error}"),
                        "log_watcher_processing_failed",
                        &error.to_string(),
                    );
                    continue;
                }
            };
            match ingestion.scan(&folders, batch).await {
                Ok(Some(scan_result)) => {
                    log_overlapping_folders(&handle, &scan_result);
                    admit_and_run(handle.clone(), pool, scan_result, SyncSource::Watcher).await;
                }
                Ok(None) => {}
                Err(error) => log_to_ui_error(
                    &handle,
                    "ERROR",
                    &format!("Watcher event processing failed: {error}"),
                    "log_watcher_processing_failed",
                    &error,
                ),
            }
        }
    });
}
