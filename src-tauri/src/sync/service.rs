//! Tauri-facing sync service facade.

use crate::{
    app::events::{emit_sync_error, log_to_ui_error},
    audit::SyncAuditContext,
    auth, db,
    immich::ImmichClient,
    sync::{queue_runner::run_sync_pipeline, SyncCoordinator},
};
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager};

pub(crate) use crate::media::events::{add_event_paths, log_overlapping_folders};
pub(crate) use crate::media::model::{Asset, FileFailure, ScanResult};
pub(crate) use crate::media::scanner::scan_folders_for_media;

pub(crate) fn ensure_sync_has_folders(folders: &[db::WatchedFolder]) -> Result<(), String> {
    if folders.is_empty() {
        return Err("Add at least one folder before starting a sync.".to_string());
    }
    Ok(())
}

pub(crate) fn create_authenticated_client(
    app: &tauri::AppHandle,
    credentials: auth::AuthConfig,
) -> Option<Arc<ImmichClient>> {
    match authenticated_client(credentials) {
        Ok(client) => Some(Arc::new(client)),
        Err(error) => {
            log_to_ui_error(
                app,
                "ERROR",
                &format!("Invalid server configuration: {}", error),
                "log_invalid_server_config",
                &error,
            );
            None
        }
    }
}

fn authenticated_client(credentials: auth::AuthConfig) -> Result<ImmichClient, String> {
    ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )
}

pub(crate) fn prepare_scan_for_queue<'a>(
    scan_result: &'a ScanResult,
    folders: &'a [db::WatchedFolder],
) -> (Vec<db::QueueAsset<'a>>, Vec<(String, String)>) {
    let assets = scan_result
        .files
        .iter()
        .map(|asset| {
            let asset_path = std::path::PathBuf::from(&asset.path);
            let target_album_id = asset.target_album_id.as_ref().or_else(|| {
                folders.iter().find_map(|f| {
                    let folder_path = std::path::PathBuf::from(&f.path);
                    if crate::media::paths::path_is_within(&asset_path, &folder_path) {
                        f.target_album_id.as_ref()
                    } else {
                        None
                    }
                })
            });
            db::QueueAsset {
                path: &asset.path,
                size: asset.size as i64,
                mtime: asset.mtime,
                target_album_id,
            }
        })
        .collect();

    let failures = scan_result
        .failures
        .iter()
        .map(|failure| (failure.path.clone(), failure.error.clone()))
        .collect();

    (assets, failures)
}

pub(crate) async fn sync_scan_result_if_authenticated(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    scan_result: ScanResult,
    source: &'static str,
) {
    log_overlapping_folders(&app, &scan_result);
    if scan_result.files.is_empty() && scan_result.failures.is_empty() {
        return;
    }

    // Persist before scheduling the runner. This keeps the watcher receiver free to
    // keep draining events while another run is active or the queue is paused.
    let folders = db::get_folders(&pool).await.unwrap_or_default();
    let (assets, failures) = prepare_scan_for_queue(&scan_result, &folders);
    match db::enqueue_sync_assets(&pool, &assets, &failures).await {
        Ok(snapshot) => {
            crate::app::tray::update_sync_action(&app, &snapshot);
            let _ = app.emit("sync-progress-snapshot", &snapshot);
        }
        Err(error) => {
            emit_sync_error(
                &app,
                &format!("Could not persist automatic sync queue: {}", error),
            );
            return;
        }
    }

    let coordinator = app.state::<SyncCoordinator>().0.clone();
    // A restored pause permits scans to add work but must never start the runner.
    if coordinator.paused.load(Ordering::SeqCst) {
        return;
    }

    if let Ok(Some(credentials)) = auth::get_credentials() {
        let audit_context = SyncAuditContext::from_credentials(&credentials, source);
        if let Some(client) = create_authenticated_client(&app, credentials) {
            let app_for_runner = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = run_sync_pipeline(
                    app_for_runner.clone(),
                    pool,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_folder_guard_rejects_an_empty_database() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{}?mode=memory&cache=private",
                database_name
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let folders = db::get_folders(&pool).await.unwrap();
        assert_eq!(
            ensure_sync_has_folders(&folders),
            Err("Add at least one folder before starting a sync.".to_string())
        );

        db::add_folder(&pool, "C:/photos", None).await.unwrap();
        let folders = db::get_folders(&pool).await.unwrap();
        assert!(ensure_sync_has_folders(&folders).is_ok());
    }

    #[test]
    fn persisted_local_http_opt_in_is_used_for_sync_clients() {
        let credentials = auth::AuthConfig {
            server_url: "http://localhost".to_string(),
            api_key: "key".to_string(),
            allow_local_http: true,
            cloudflare_client_id: None,
            cloudflare_client_secret: None,
        };

        assert!(authenticated_client(credentials.clone()).is_ok());
        let denied_credentials = auth::AuthConfig {
            allow_local_http: false,
            ..credentials
        };
        assert!(authenticated_client(denied_credentials).is_err());
    }
}
