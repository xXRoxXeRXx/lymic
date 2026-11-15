use crate::watched_folders::{
    RegisterFolderParams, RemoveFolderResult, WatchedFolderRegistration, WatchedFolderRemoval,
};
use crate::{
    db,
    sync::{admit_and_run, scan_watched_folders_for_media, SyncCoordinator, SyncSource},
    watcher,
};
use tauri::Manager;

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
    let running = app.state::<crate::sync::SyncState>();
    WatchedFolderRemoval::for_tauri(
        &app,
        pool.inner(),
        watcher.inner(),
        running.inner(),
        sync_coordinator.inner(),
    )
    .remove(id, confirm_cancellation)
    .await
}

#[cfg(test)]
mod tests {
    use crate::watched_folders::{removal_requires_confirmation, RemoveFolderResult};

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
