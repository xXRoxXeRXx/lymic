//! Thin Tauri adapters for the synchronization application service.

use crate::{
    db,
    sync::{SyncRuntime, Synchronization},
};

#[tauri::command]
pub(crate) async fn start_sync(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, SyncRuntime>,
) -> Result<crate::sync::SyncSummary, String> {
    Synchronization::new(&app, runtime.inner()).start().await
}

#[tauri::command]
pub(crate) async fn get_sync_status(
    runtime: tauri::State<'_, SyncRuntime>,
) -> Result<db::SyncSnapshot, String> {
    runtime.status().await.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn pause_sync(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, SyncRuntime>,
) -> Result<db::SyncSnapshot, String> {
    Synchronization::new(&app, runtime.inner()).pause().await
}

#[tauri::command]
pub(crate) async fn resume_sync(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, SyncRuntime>,
) -> Result<db::SyncSnapshot, String> {
    Synchronization::new(&app, runtime.inner()).resume().await
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
    runtime: tauri::State<'_, SyncRuntime>,
) -> Result<crate::sync::SyncSummary, String> {
    Synchronization::new(&app, runtime.inner()).retry().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

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
