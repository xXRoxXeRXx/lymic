use crate::db;
use crate::watched_folders::{RegisterFolderParams, RemoveFolderResult, WatchedFolders};

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
pub(crate) async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    path: String,
    target_album_id: Option<String>,
) -> Result<i64, String> {
    use tauri::Manager;
    let active_volumes = app.try_state::<crate::removable::ActiveRemovableVolumes>();
    let input =
        RegisterFolderParams::from_local_path(path, target_album_id, active_volumes.as_deref());

    WatchedFolders::for_tauri(&app, pool.inner())
        .register(input)
        .await
}

#[tauri::command]
pub(crate) async fn accept_removable_media_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
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

    WatchedFolders::for_tauri(&app, pool.inner())
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
    WatchedFolders::for_tauri(&app, pool.inner())
        .update_exclusion_patterns(id, exclusion_patterns)
        .await
}

#[tauri::command]
pub(crate) async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    id: i64,
    confirm_cancellation: Option<bool>,
) -> Result<RemoveFolderResult, String> {
    WatchedFolders::for_tauri(&app, pool.inner())
        .remove(id, confirm_cancellation)
        .await
}
