//! Thin Tauri adapters for the synchronization application service.

use crate::{
    db,
    sync::{load_upload_parallelism, SyncCoordinator, SyncState, Synchronization},
};

fn synchronization<'a>(
    app: &'a tauri::AppHandle,
    pool: &'a sqlx::SqlitePool,
    sync_state: &'a SyncState,
    coordinator: &'a SyncCoordinator,
) -> Synchronization<'a> {
    Synchronization::new(app, pool, &sync_state.0, &coordinator.0)
}

#[tauri::command]
pub(crate) async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<crate::sync::SyncSummary, String> {
    synchronization(
        &app,
        pool.inner(),
        sync_state.inner(),
        sync_coordinator.inner(),
    )
    .start()
    .await
}

#[tauri::command]
pub(crate) async fn get_sync_status(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    synchronization(
        &app,
        pool.inner(),
        sync_state.inner(),
        sync_coordinator.inner(),
    )
    .status()
    .await
}

#[tauri::command]
pub(crate) async fn pause_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    pause_sync_inner(&app, pool.inner(), &sync_coordinator.inner().0).await
}

pub(crate) async fn pause_sync_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    coordinator: &std::sync::Arc<crate::sync::SyncCoordinatorInner>,
) -> Result<db::SyncSnapshot, String> {
    let running = std::sync::atomic::AtomicBool::new(false);
    Synchronization::new(app, pool, &running, coordinator)
        .pause()
        .await
}

#[tauri::command]
pub(crate) async fn resume_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    resume_sync_inner(&app, pool.inner(), &sync_coordinator.inner().0).await
}

pub(crate) async fn resume_sync_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    coordinator: &std::sync::Arc<crate::sync::SyncCoordinatorInner>,
) -> Result<db::SyncSnapshot, String> {
    let running = std::sync::atomic::AtomicBool::new(false);
    Synchronization::new(app, pool, &running, coordinator)
        .resume()
        .await
}

#[tauri::command]
pub(crate) async fn get_failed_syncs(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::FailedSyncEntry>, String> {
    db::get_failed_syncs(pool.inner())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn get_upload_parallelism(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<usize, String> {
    load_upload_parallelism(pool.inner()).await
}

#[tauri::command]
pub(crate) async fn set_upload_parallelism(
    upload_parallelism: usize,
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<(), String> {
    db::set_upload_parallelism(pool.inner(), upload_parallelism).await
}

#[tauri::command]
pub(crate) async fn retry_failed_syncs(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<crate::sync::SyncSummary, String> {
    synchronization(
        &app,
        pool.inner(),
        sync_state.inner(),
        sync_coordinator.inner(),
    )
    .retry()
    .await
}
