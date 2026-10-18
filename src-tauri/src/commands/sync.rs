//! Thin Tauri adapters for the synchronization application service.

use crate::{
    db,
    sync::{SyncCoordinator, SyncState, Synchronization},
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
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    pause_sync_inner(
        &app,
        pool.inner(),
        &sync_state.inner().0,
        &sync_coordinator.inner().0,
    )
    .await
}

pub(crate) async fn pause_sync_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    running: &std::sync::atomic::AtomicBool,
    coordinator: &std::sync::Arc<crate::sync::SyncCoordinatorInner>,
) -> Result<db::SyncSnapshot, String> {
    Synchronization::new(app, pool, running, coordinator)
        .pause()
        .await
}

#[tauri::command]
pub(crate) async fn resume_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    resume_sync_inner(
        &app,
        pool.inner(),
        &sync_state.inner().0,
        &sync_coordinator.inner().0,
    )
    .await
}

pub(crate) async fn resume_sync_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    running: &std::sync::atomic::AtomicBool,
    coordinator: &std::sync::Arc<crate::sync::SyncCoordinatorInner>,
) -> Result<db::SyncSnapshot, String> {
    Synchronization::new(app, pool, running, coordinator)
        .resume()
        .await
}

#[tauri::command]
pub(crate) async fn get_failed_syncs(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::FailedSyncEntry>, String> {
    get_failed_syncs_inner(pool.inner()).await
}

async fn get_failed_syncs_inner(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<db::FailedSyncEntry>, String> {
    db::get_failed_syncs(pool)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn get_upload_parallelism(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<usize, String> {
    get_upload_parallelism_inner(pool.inner()).await
}

async fn get_upload_parallelism_inner(pool: &sqlx::SqlitePool) -> Result<usize, String> {
    crate::sync::load_upload_parallelism(pool).await
}

#[tauri::command]
pub(crate) async fn set_upload_parallelism(
    upload_parallelism: usize,
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<(), String> {
    set_upload_parallelism_inner(pool.inner(), upload_parallelism).await
}

async fn set_upload_parallelism_inner(
    pool: &sqlx::SqlitePool,
    upload_parallelism: usize,
) -> Result<(), String> {
    db::set_upload_parallelism(pool, upload_parallelism).await
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> sqlx::SqlitePool {
        let database_name = format!("lymic_command_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn settings_command_adapters_preserve_parallelism_values_and_validation() {
        let pool = test_pool().await;

        assert_eq!(get_upload_parallelism_inner(&pool).await.unwrap(), 3);
        set_upload_parallelism_inner(&pool, 6).await.unwrap();
        assert_eq!(get_upload_parallelism_inner(&pool).await.unwrap(), 6);
        assert_eq!(
            set_upload_parallelism_inner(&pool, 9).await,
            Err("Upload parallelism must be between 1 and 8.".to_string())
        );
    }

    #[tokio::test]
    async fn failed_syncs_command_adapter_returns_persisted_failure_details() {
        let pool = test_pool().await;
        db::mark_sync_failed(&pool, "C:/photos/failed.jpg", 1, 2, "network unavailable")
            .await
            .unwrap();

        let failures = get_failed_syncs_inner(&pool).await.unwrap();

        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "C:/photos/failed.jpg");
        assert_eq!(failures[0].failure_reason, "network unavailable");
    }
}
