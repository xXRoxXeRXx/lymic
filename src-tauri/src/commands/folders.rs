use crate::app::events::{log_to_ui_error, log_to_ui_folder_error};
use crate::app::tray::set_sync_enabled;
use crate::audit::audit_event;
use crate::watched_folders::{RegisterFolderParams, WatchedFolderRegistration};
use crate::{
    db,
    sync::{admit_and_run, scan_watched_folders_for_media, SyncCoordinator, SyncSource},
    watcher,
};
use serde_json::json;
use std::path::PathBuf;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveFolderResult {
    requires_sync_cancellation: bool,
}

fn removal_requires_confirmation(confirm_cancellation: Option<bool>) -> bool {
    !confirm_cancellation.unwrap_or(false)
}

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
pub(crate) async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    path: String,
    target_album_id: Option<String>,
) -> Result<i64, String> {
    use tauri::Manager;
    let active_volumes = app.try_state::<crate::removable::ActiveRemovableVolumes>();
    let input =
        RegisterFolderParams::from_local_path(path, target_album_id, active_volumes.as_deref());

    WatchedFolderRegistration::for_tauri(&app, pool.inner(), watcher.inner())
        .register(input)
        .await
}

#[tauri::command]
pub(crate) async fn accept_removable_media_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    active_volumes: tauri::State<'_, crate::removable::ActiveRemovableVolumes>,
    volume_id: String,
    path: String,
    target_album_id: Option<String>,
) -> Result<i64, String> {
    let input = RegisterFolderParams::from_removable_acceptance(
        active_volumes.inner(),
        volume_id,
        path,
        target_album_id,
    )?;

    WatchedFolderRegistration::for_tauri(&app, pool.inner(), watcher.inner())
        .register(input)
        .await
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
pub(crate) async fn update_folder_exclusion_patterns(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    id: i64,
    exclusion_patterns: Vec<String>,
) -> Result<(), String> {
    let exclusion_patterns = crate::media::admission::normalize_patterns(exclusion_patterns)?;
    if !db::update_exclusion_patterns(pool.inner(), id, &exclusion_patterns)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("Watched folder was not found.".to_string());
    }
    let folder = db::get_folders(pool.inner())
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|folder| folder.id == id)
        .ok_or("Watched folder was not found.")?;
    let scan_result = scan_watched_folders_for_media(vec![folder]).await?;
    admit_and_run(
        app,
        pool.inner().clone(),
        scan_result,
        SyncSource::ExclusionPatternsChanged,
    )
    .await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
    id: i64,
    confirm_cancellation: Option<bool>,
) -> Result<RemoveFolderResult, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let path = folder.path.clone();
        let has_active_work = sync_coordinator.0.active_work.has_work_in(&path)
            || db::has_sync_work_in_folder(pool.inner(), &path)
                .await
                .map_err(|error| error.to_string())?;
        if removal_requires_confirmation(confirm_cancellation) {
            return Ok(RemoveFolderResult {
                requires_sync_cancellation: has_active_work,
            });
        }
        {
            let mut w = watcher.lock().await;
            if let Err(e) = watcher::unwatch_path(&mut w, &path) {
                log_to_ui_folder_error(
                    &app,
                    "WARN",
                    &format!("Failed to unwatch '{}': {}", path, e),
                    "log_unwatch_failed",
                    &path,
                    &e.to_string(),
                );
                return Err(format!("Failed to unwatch '{}': {}", path, e));
            }
        }

        // The watcher is gone before cancellation so no new work can be produced for this root.
        // The queue cleanup is path-component based and leaves sibling roots untouched.
        sync_coordinator.0.active_work.cancel_work_in(&path);
        if let Err(error) = db::discard_sync_data_in_folder(pool.inner(), &path).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            let _ = w.reconcile_path(&path, is_available);
            return Err(error.to_string());
        }

        if let Err(error) = db::remove_folder(pool.inner(), id).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            if let Err(rewatch_error) = w.reconcile_path(&path, is_available) {
                log_to_ui_folder_error(
                    &app,
                    "ERROR",
                    &format!(
                        "Could not restore watcher registration for '{}': {}",
                        path, rewatch_error
                    ),
                    "log_watcher_restore_failed",
                    &path,
                    &rewatch_error.to_string(),
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
        Err(error) => log_to_ui_error(
            &app,
            "ERROR",
            &format!(
                "Folder was removed, but could not refresh tray sync availability: {}",
                error
            ),
            "log_tray_refresh_after_remove_failed",
            &error.to_string(),
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
    Ok(RemoveFolderResult {
        requires_sync_cancellation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{removal_requires_confirmation, RemoveFolderResult};

    #[test]
    fn folder_removal_requires_explicit_confirmation() {
        assert!(removal_requires_confirmation(None));
        assert!(removal_requires_confirmation(Some(false)));
        assert!(!removal_requires_confirmation(Some(true)));

        let result = RemoveFolderResult {
            requires_sync_cancellation: true,
        };
        assert!(result.requires_sync_cancellation);
    }

    #[test]
    fn exclusion_patterns_are_normalized_deduplicated_and_validated() {
        assert_eq!(
            crate::media::admission::normalize_patterns(vec![
                " Screenshots/** ".into(),
                "screenshots/**".into(),
                "".into()
            ])
            .unwrap(),
            vec!["Screenshots/**"]
        );
        assert!(crate::media::admission::normalize_patterns(vec!["../private/**".into()]).is_err());
        for invalid in ["Screenshots\\**", "/private/**", "!keep.jpg", "*.{jpg,png}"] {
            assert!(crate::media::admission::normalize_patterns(vec![invalid.into()]).is_err());
        }
    }
}
