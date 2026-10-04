use crate::app::events::log_to_ui;
use crate::app::startup::StartupContext;
use crate::{
    audit::{self, audit_event},
    db,
    sync::{
        add_event_paths, log_overlapping_folders, scan_folders_for_media,
        sync_scan_result_if_authenticated, Asset, FileFailure, ScanResult,
    },
};
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::Manager;

pub(crate) fn spawn(handle: tauri::AppHandle, startup: StartupContext) {
    spawn_deferred_watches(handle.clone());
    spawn_startup_sync(handle.clone());
    spawn_watcher_consumer(
        handle,
        startup.watch_events,
        startup.rescan_requested,
        startup.rescan_notify,
    );
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
                    log_to_ui(
                        &handle,
                        "ERROR",
                        &format!("Could not load deferred watched folders: {error}"),
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
                    log_to_ui(
                        &handle,
                        "ERROR",
                        &format!("Could not check deferred watched folders: {error}"),
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
                                log_to_ui(
                                    &handle,
                                    "WARN",
                                    &format!("Could not watch '{}': {}", path.display(), error),
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
                log_to_ui(
                    &handle,
                    "INFO",
                    &format!(
                        "Folder '{}' is available; watching enabled.",
                        path.display()
                    ),
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
            let scan_result = match scan_folders_for_media(registered_paths).await {
                Ok(scan_result) => scan_result,
                Err(error) => {
                    log_to_ui(&handle, "ERROR", &error);
                    continue;
                }
            };
            sync_scan_result_if_authenticated(handle.clone(), pool, scan_result, "deferred_folder")
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
                log_to_ui(
                    &handle,
                    "ERROR",
                    &format!("Could not load watched folders for startup sync: {error}"),
                );
                return;
            }
        };
        for folder in &folders {
            log_to_ui(
                &handle,
                "INFO",
                &format!("Scanning folder: {}", folder.path),
            );
        }
        let scan_result = match scan_folders_for_media(
            folders
                .into_iter()
                .map(|folder| std::path::PathBuf::from(folder.path))
                .collect(),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                log_to_ui(&handle, "ERROR", &error);
                return;
            }
        };
        sync_scan_result_if_authenticated(handle.clone(), pool, scan_result, "startup_sync").await;
    });
}

fn spawn_watcher_consumer(
    handle: tauri::AppHandle,
    mut watch_events: tokio::sync::mpsc::Receiver<notify::Event>,
    rescan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    rescan_notify: std::sync::Arc<tokio::sync::Notify>,
) {
    tauri::async_runtime::spawn(async move {
        let mut files_buffer = std::collections::HashMap::<String, Asset>::new();
        let mut failures_buffer = std::collections::HashMap::<String, FileFailure>::new();
        loop {
            tokio::select! {
                Some(event) = watch_events.recv() => if let Err(error) = add_event_paths(&mut files_buffer, &mut failures_buffer, event).await { log_to_ui(&handle, "ERROR", &error); },
                _ = rescan_notify.notified() => {}
            }
            let debounce = tokio::time::sleep(tokio::time::Duration::from_secs(2));
            tokio::pin!(debounce);
            loop {
                tokio::select! {
                    Some(event) = watch_events.recv() => {
                        if let Err(error) = add_event_paths(&mut files_buffer, &mut failures_buffer, event).await { log_to_ui(&handle, "ERROR", &error); }
                        debounce.as_mut().reset(tokio::time::Instant::now() + tokio::time::Duration::from_secs(2));
                    }
                    _ = rescan_notify.notified() => {}
                    _ = &mut debounce => break,
                }
            }
            if rescan_requested.swap(false, Ordering::AcqRel) {
                log_to_ui(
                    &handle,
                    "WARN",
                    "File watcher event queue overflowed; reconciling all watched folders.",
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
                files_buffer.clear();
                failures_buffer.clear();
                let pool = handle.state::<sqlx::SqlitePool>();
                match db::get_folders(pool.inner()).await {
                    Ok(folders) => match scan_folders_for_media(
                        folders
                            .into_iter()
                            .map(|folder| std::path::PathBuf::from(folder.path))
                            .collect(),
                    )
                    .await
                    {
                        Ok(scan_result) => {
                            log_overlapping_folders(&handle, &scan_result);
                            for asset in scan_result.files {
                                files_buffer.insert(asset.path.clone(), asset);
                            }
                            for failure in scan_result.failures {
                                failures_buffer.insert(failure.path.clone(), failure);
                            }
                        }
                        Err(error) => log_to_ui(&handle, "ERROR", &error),
                    },
                    Err(error) => log_to_ui(
                        &handle,
                        "ERROR",
                        &format!("Could not reconcile watched folders after event loss: {error}"),
                    ),
                }
            }
            if !files_buffer.is_empty() || !failures_buffer.is_empty() {
                let mut scan_result = ScanResult::default();
                scan_result
                    .files
                    .extend(files_buffer.drain().map(|(_, asset)| asset));
                scan_result
                    .failures
                    .extend(failures_buffer.drain().map(|(_, failure)| failure));
                let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                sync_scan_result_if_authenticated(
                    handle.clone(),
                    pool,
                    scan_result,
                    "watcher_sync",
                )
                .await;
            }
        }
    });
}
