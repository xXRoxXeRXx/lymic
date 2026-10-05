use crate::app::events::{emit_sync_error, log_to_ui};
use crate::audit::{audit_event, audit_safe_error, SyncAuditContext};
use crate::sync::{SyncCoordinator, SyncState};
use crate::{
    auth, db,
    sync::{
        create_authenticated_client, ensure_sync_has_folders, load_upload_parallelism,
        log_overlapping_folders, prepare_failed_sync_retry, run_sync_pipeline,
        scan_folders_for_media, ScanResult, SyncSummary,
    },
};
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::Emitter;

#[tauri::command]
pub(crate) async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<SyncSummary, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    ensure_sync_has_folders(&folders)?;

    let command_operation_id = crate::audit::AuditEvent::operation_id();
    // Attempt to set sync_state to true. If it was already true, return early.
    if sync_state
        .0
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log_to_ui(
            &app,
            "WARN",
            "Manual sync already in progress. Ignoring request.",
        );
        return Err("Sync already in progress".to_string());
    }

    log_to_ui(&app, "INFO", "Starting manual synchronization...");
    // Ensure we reset the state when we're done, even if we fail.
    let result = async {
        let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
        let audit_context = SyncAuditContext::from_credentials(
            &credentials.server_url,
            &credentials.api_key,
            "manual_sync",
        );
        let client =
            create_authenticated_client(&app, credentials).ok_or("Invalid server configuration")?;

        for folder in &folders {
            log_to_ui(&app, "INFO", &format!("Scanning folder: {}", folder.path));
        }
        let folder_paths = folders
            .into_iter()
            .map(|folder| std::path::PathBuf::from(folder.path))
            .collect();
        let scan_result = scan_folders_for_media(folder_paths).await?;
        log_overlapping_folders(&app, &scan_result);

        run_sync_pipeline(
            app.clone(),
            pool.inner().clone(),
            client,
            scan_result,
            false,
            sync_coordinator.0.clone(),
            audit_context,
        )
        .await
    }
    .await;

    sync_state.0.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        audit_event(
            &app,
            crate::audit::AuditEvent::new(
                &command_operation_id,
                "sync.aborted",
                crate::audit::Outcome::Failure,
                crate::audit::Severity::Error,
                None,
                "manual_sync",
                "Manual synchronization aborted",
                json!({ "failure_reason": audit_safe_error(error, None, None) }),
            ),
        );
    }
    result
}

#[tauri::command]
pub(crate) async fn get_sync_status(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<db::SyncSnapshot, String> {
    db::get_sync_snapshot(pool.inner())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn pause_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    sync_coordinator.0.paused.store(true, Ordering::SeqCst);
    // A runner changes this to PAUSED at its next safe block boundary. Persisting it
    // immediately also makes a close/crash between request and boundary safe.
    let snapshot = db::set_sync_status(pool.inner(), "PAUSED")
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("sync-paused", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn resume_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<db::SyncSnapshot, String> {
    let snapshot = db::get_sync_snapshot(pool.inner())
        .await
        .map_err(|error| error.to_string())?;
    if snapshot.status != "PAUSED" {
        return Ok(snapshot);
    }
    sync_coordinator.0.paused.store(false, Ordering::SeqCst);
    let snapshot = db::set_sync_status(pool.inner(), "RUNNING")
        .await
        .map_err(|error| error.to_string())?;
    sync_coordinator.0.resume.notify_waiters();
    let _ = app.emit("sync-resumed", &snapshot);

    // If no in-memory runner owns the lock, this is recovered work. Rebuild the
    // scan result from the persisted queue rather than rescanning watched folders.
    if let Ok(guard) = sync_coordinator.0.lock.try_lock() {
        drop(guard);
        let has_pending = db::has_pending_sync_queue_items(pool.inner())
            .await
            .map_err(|error| error.to_string())?;
        if has_pending {
            let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
            let audit_context = SyncAuditContext::from_credentials(
                &credentials.server_url,
                &credentials.api_key,
                "resume_sync",
            );
            let client = create_authenticated_client(&app, credentials)
                .ok_or("Invalid server configuration")?;
            audit_event(
                &app,
                audit_context.event(
                    "sync.resumed",
                    crate::audit::Outcome::Info,
                    crate::audit::Severity::Info,
                    "Synchronization resumed",
                    json!({}),
                ),
            );
            let app_for_runner = app.clone();
            let pool_for_runner = pool.inner().clone();
            let coordinator = sync_coordinator.0.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = run_sync_pipeline(
                    app_for_runner.clone(),
                    pool_for_runner,
                    client,
                    ScanResult::default(),
                    true,
                    coordinator,
                    audit_context,
                )
                .await
                {
                    emit_sync_error(&app_for_runner, &error);
                }
            });
        } else {
            return db::complete_sync_job_if_finished(pool.inner())
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Sync queue is still processing.".to_string());
        }
    }
    Ok(snapshot)
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
) -> Result<SyncSummary, String> {
    if sync_state
        .0
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Sync already in progress".to_string());
    }

    let command_operation_id = crate::audit::AuditEvent::operation_id();
    let result = async {
        let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
        let audit_context = SyncAuditContext::from_credentials(
            &credentials.server_url,
            &credentials.api_key,
            "retry_failed_syncs",
        );
        let client =
            create_authenticated_client(&app, credentials).ok_or("Invalid server configuration")?;
        let failed_paths = db::get_failed_syncs(pool.inner())
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.local_path)
            .collect();
        let scan_result = prepare_failed_sync_retry(failed_paths).await?;
        db::discard_failed_sync_paths(pool.inner(), &scan_result.discarded_paths)
            .await
            .map_err(|error| error.to_string())?;

        run_sync_pipeline(
            app.clone(),
            pool.inner().clone(),
            client,
            scan_result,
            false,
            sync_coordinator.0.clone(),
            audit_context,
        )
        .await
    }
    .await;

    sync_state.0.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        audit_event(
            &app,
            crate::audit::AuditEvent::new(
                &command_operation_id,
                "sync.retry_aborted",
                crate::audit::Outcome::Failure,
                crate::audit::Severity::Error,
                None,
                "retry_failed_syncs",
                "Failed synchronization retry aborted",
                json!({ "failure_reason": audit_safe_error(error, None, None) }),
            ),
        );
    }
    result
}
