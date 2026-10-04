use crate::app::events::log_to_ui;
use crate::app::tray::set_sync_enabled;
use crate::audit::audit_event;
use crate::media::paths::{normalize_folder_path, path_is_within};
use crate::{
    db,
    sync::{scan_folders_for_media, sync_scan_result_if_authenticated},
    watcher,
};
use serde_json::json;
use std::path::PathBuf;

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
pub(crate) async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    path: String,
) -> Result<i64, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|error| error.to_string())?;
    let path_for_normalization = PathBuf::from(&path);
    let existing_paths = folders
        .into_iter()
        .map(|folder| PathBuf::from(folder.path))
        .collect::<Vec<_>>();
    let (normalized_path, conflict) = tokio::task::spawn_blocking(move || {
        let normalized_path = normalize_folder_path(&path_for_normalization);
        let conflict = existing_paths
            .into_iter()
            .map(|path| normalize_folder_path(&path))
            .find_map(|existing_path| {
                let is_duplicate = path_is_within(&normalized_path, &existing_path)
                    && path_is_within(&existing_path, &normalized_path);
                if is_duplicate
                    || path_is_within(&normalized_path, &existing_path)
                    || path_is_within(&existing_path, &normalized_path)
                {
                    Some((existing_path, is_duplicate))
                } else {
                    None
                }
            });
        (normalized_path, conflict)
    })
    .await
    .map_err(|error| format!("Could not normalize folder '{}': {}", path, error))?;
    let normalized_path_string = normalized_path.to_string_lossy().to_string();

    if let Some((existing_path, is_duplicate)) = conflict {
        let message = if is_duplicate {
            format!(
                "Folder '{}' is already being watched.",
                normalized_path.display()
            )
        } else {
            format!(
                "Folder '{}' overlaps with already watched folder '{}'. Remove one of the folders before adding it.",
                normalized_path.display(),
                existing_path.display()
            )
        };
        return Err(message);
    }

    let path = normalized_path_string;
    let path_for_check = PathBuf::from(&path);
    let is_available = match tokio::task::spawn_blocking(move || {
        std::fs::metadata(path_for_check).map(|metadata| metadata.is_dir())
    })
    .await
    {
        Ok(Ok(true)) => true,
        Ok(Ok(false)) => return Err(format!("Path '{}' is not a directory.", path)),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
        Ok(Err(error)) => return Err(format!("Could not access folder '{}': {}", path, error)),
        Err(error) => return Err(format!("Could not check folder '{}': {}", path, error)),
    };

    let registration = if is_available {
        let mut w = watcher.lock().await;
        w.reconcile_path(&path, true)
            .map_err(|error| format!("Could not watch '{}': {}", path, error))?
    } else {
        watcher::WatchRegistration::PathUnavailable
    };

    let id = match db::add_folder(pool.inner(), &path).await {
        Ok(id) => id,
        Err(error) => {
            if registration == watcher::WatchRegistration::Registered {
                let mut w = watcher.lock().await;
                if let Err(unwatch_error) = watcher::unwatch_path(&mut w, &path) {
                    log_to_ui(
                        &app,
                        "ERROR",
                        &format!(
                            "Could not undo watcher registration for '{}': {}",
                            path, unwatch_error
                        ),
                    );
                }
            }
            return Err(error.to_string());
        }
    };

    // The persisted folder is the source of truth, even when its watcher is deferred.
    set_sync_enabled(&app, true);

    if registration == watcher::WatchRegistration::PathUnavailable {
        log_to_ui(
            &app,
            "WARN",
            &format!(
                "Folder '{}' does not exist; watching deferred until it appears.",
                path
            ),
        );
    }

    // An offline folder will be scanned when the deferred-watch task registers it.
    if is_available {
        let pool_inner = pool.inner().clone();
        let path_clone = path.clone();
        let app_for_scan = app.clone();
        tauri::async_runtime::spawn(async move {
            let scan_result = match scan_folders_for_media(vec![PathBuf::from(path_clone)]).await {
                Ok(result) => result,
                Err(error) => {
                    log_to_ui(&app_for_scan, "ERROR", &error);
                    return;
                }
            };
            sync_scan_result_if_authenticated(app_for_scan, pool_inner, scan_result, "folder_add")
                .await;
        });
    }

    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            crate::audit::AuditEvent::operation_id(),
            "folder.added",
            crate::audit::Outcome::Success,
            crate::audit::Severity::Info,
            None,
            "command",
            "Watched folder added",
            json!({ "folder_id": id, "local_path": path }),
        ),
    );
    Ok(id)
}

#[tauri::command]
pub(crate) async fn get_folders(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::WatchedFolder>, String> {
    db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    id: i64,
) -> Result<(), String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let path = folder.path.clone();
        {
            let mut w = watcher.lock().await;
            if let Err(e) = watcher::unwatch_path(&mut w, &path) {
                log_to_ui(
                    &app,
                    "WARN",
                    &format!("Failed to unwatch '{}': {}", path, e),
                );
                return Err(format!("Failed to unwatch '{}': {}", path, e));
            }
        }

        if let Err(error) = db::remove_folder(pool.inner(), id).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            if let Err(rewatch_error) = w.reconcile_path(&path, is_available) {
                log_to_ui(
                    &app,
                    "ERROR",
                    &format!(
                        "Could not restore watcher registration for '{}': {}",
                        path, rewatch_error
                    ),
                );
            }
            return Err(error.to_string());
        }
    } else {
        db::remove_folder(pool.inner(), id)
            .await
            .map_err(|e| e.to_string())?;
    }
    match db::get_folders(pool.inner()).await {
        Ok(folders) => set_sync_enabled(&app, !folders.is_empty()),
        Err(error) => log_to_ui(
            &app,
            "ERROR",
            &format!(
                "Folder was removed, but could not refresh tray sync availability: {}",
                error
            ),
        ),
    }
    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            crate::audit::AuditEvent::operation_id(),
            "folder.removed",
            crate::audit::Outcome::Success,
            crate::audit::Severity::Info,
            None,
            "command",
            "Watched folder removed",
            json!({ "folder_id": id }),
        ),
    );
    Ok(())
}
