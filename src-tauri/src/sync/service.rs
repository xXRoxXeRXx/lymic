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

/// Durable synchronization state transitions, independent of the desktop shell.
///
/// `Synchronization` owns the surrounding desktop orchestration; this smaller
/// collaborator is deliberately kept free of Tauri so its state-machine
/// contract can be exercised against the real SQLite schema.
pub(crate) struct SynchronizationLifecycle<'a> {
    pool: &'a sqlx::SqlitePool,
    running: &'a std::sync::atomic::AtomicBool,
    coordinator: Arc<crate::sync::SyncCoordinatorInner>,
}

impl<'a> SynchronizationLifecycle<'a> {
    pub(crate) fn new(
        pool: &'a sqlx::SqlitePool,
        running: &'a std::sync::atomic::AtomicBool,
        coordinator: Arc<crate::sync::SyncCoordinatorInner>,
    ) -> Self {
        Self {
            pool,
            running,
            coordinator,
        }
    }

    pub(crate) async fn start(&self) -> Result<db::SyncSnapshot, String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Sync already in progress".to_string());
        }

        match db::set_sync_status(self.pool, "RUNNING").await {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                Err(error.to_string())
            }
        }
    }

    pub(crate) fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub(crate) async fn abort(&self) -> Result<db::SyncSnapshot, String> {
        self.finish();
        db::set_sync_status(self.pool, "IDLE")
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, String> {
        self.coordinator.paused.store(true, Ordering::SeqCst);
        db::set_sync_status(self.pool, "PAUSED")
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, String> {
        let snapshot = self.status().await?;
        if snapshot.status != "PAUSED" {
            return Ok(snapshot);
        }

        self.coordinator.paused.store(false, Ordering::SeqCst);
        let snapshot = db::set_sync_status(self.pool, "RUNNING")
            .await
            .map_err(|error| error.to_string())?;
        self.coordinator.resume.notify_waiters();
        Ok(snapshot)
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, String> {
        db::get_sync_snapshot(self.pool)
            .await
            .map_err(|error| error.to_string())
    }
}

/// The synchronization application service.
///
/// It is the one entry point for a synchronization run and localizes the
/// desktop effects around the durable lifecycle transitions.
pub(crate) struct Synchronization<'a> {
    app: &'a tauri::AppHandle,
    pool: &'a sqlx::SqlitePool,
    running: &'a std::sync::atomic::AtomicBool,
    coordinator: &'a Arc<crate::sync::SyncCoordinatorInner>,
}

impl<'a> Synchronization<'a> {
    pub(crate) fn new(
        app: &'a tauri::AppHandle,
        pool: &'a sqlx::SqlitePool,
        running: &'a std::sync::atomic::AtomicBool,
        coordinator: &'a Arc<crate::sync::SyncCoordinatorInner>,
    ) -> Self {
        Self {
            app,
            pool,
            running,
            coordinator,
        }
    }

    fn lifecycle(&self) -> SynchronizationLifecycle<'_> {
        SynchronizationLifecycle::new(self.pool, self.running, self.coordinator.clone())
    }

    pub(crate) async fn start(&self) -> Result<crate::sync::SyncSummary, String> {
        let folders = db::get_folders(self.pool)
            .await
            .map_err(|error| error.to_string())?;
        ensure_sync_has_folders(&folders)?;
        let operation_id = crate::audit::AuditEvent::operation_id();
        let running_snapshot = match self.lifecycle().start().await {
            Ok(snapshot) => snapshot,
            Err(error) if error == "Sync already in progress" => {
                crate::app::events::log_to_ui_key(
                    self.app,
                    "WARN",
                    "Manual sync already in progress. Ignoring request.",
                    "log_sync_already_in_progress",
                );
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        crate::app::tray::update_sync_action(self.app, &running_snapshot);
        crate::app::events::log_to_ui_key(
            self.app,
            "INFO",
            "Starting manual synchronization...",
            "log_starting_sync",
        );

        let result = async {
            let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
            let audit_context = SyncAuditContext::from_credentials(&credentials, "manual_sync");
            let client = create_authenticated_client(self.app, credentials)
                .ok_or("Invalid server configuration")?;
            for folder in &folders {
                crate::app::events::log_to_ui_folder(
                    self.app,
                    "INFO",
                    &format!("Scanning folder: {}", folder.path),
                    "log_scanning_folder",
                    &folder.path,
                );
            }
            let scan_result = scan_watched_folders_for_media(folders).await?;
            log_overlapping_folders(self.app, &scan_result);
            crate::sync::queue_runner::run_sync_pipeline(
                self.app.clone(),
                self.pool.clone(),
                client,
                scan_result,
                false,
                self.coordinator.clone(),
                audit_context,
            )
            .await
        }
        .await;

        self.lifecycle().finish();
        if let Err(error) = &result {
            if let Ok(snapshot) = self.lifecycle().abort().await {
                crate::app::tray::update_sync_action(self.app, &snapshot);
            }
            crate::audit::audit_event(
                self.app,
                crate::audit::AuditEvent::new(
                    &operation_id,
                    "sync.aborted",
                    crate::audit::Outcome::Failure,
                    crate::audit::Severity::Error,
                    None,
                    "manual_sync",
                    "Manual synchronization aborted",
                    serde_json::json!({ "failure_reason": crate::audit::audit_safe_error(error, &[]) }),
                ),
            );
        }
        result
    }

    pub(crate) async fn retry(&self) -> Result<crate::sync::SyncSummary, String> {
        let operation_id = crate::audit::AuditEvent::operation_id();
        let running_snapshot = self.lifecycle().start().await?;
        crate::app::tray::update_sync_action(self.app, &running_snapshot);
        let result = async {
            let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
            let audit_context =
                SyncAuditContext::from_credentials(&credentials, "retry_failed_syncs");
            let client = create_authenticated_client(self.app, credentials)
                .ok_or("Invalid server configuration")?;
            let failed_paths = db::get_failed_syncs(self.pool)
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|entry| entry.local_path)
                .collect();
            let scan_result = crate::sync::prepare_failed_sync_retry(failed_paths).await?;
            db::discard_failed_sync_paths(self.pool, &scan_result.discarded_paths)
                .await
                .map_err(|error| error.to_string())?;
            crate::sync::queue_runner::run_sync_pipeline(
                self.app.clone(),
                self.pool.clone(),
                client,
                scan_result,
                false,
                self.coordinator.clone(),
                audit_context,
            )
            .await
        }
        .await;
        self.lifecycle().finish();
        if let Err(error) = &result {
            if let Ok(snapshot) = self.lifecycle().abort().await {
                crate::app::tray::update_sync_action(self.app, &snapshot);
            }
            crate::audit::audit_event(
                self.app,
                crate::audit::AuditEvent::new(
                    &operation_id,
                    "sync.retry_aborted",
                    crate::audit::Outcome::Failure,
                    crate::audit::Severity::Error,
                    None,
                    "retry_failed_syncs",
                    "Failed synchronization retry aborted",
                    serde_json::json!({ "failure_reason": crate::audit::audit_safe_error(error, &[]) }),
                ),
            );
        }
        result
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, String> {
        let snapshot = self.lifecycle().pause().await?;
        crate::app::tray::update_sync_action(self.app, &snapshot);
        let _ = self.app.emit("sync-paused", &snapshot);
        Ok(snapshot)
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, String> {
        let previous = self.lifecycle().status().await?;
        if previous.status != "PAUSED" {
            return Ok(previous);
        }
        let snapshot = self.lifecycle().resume().await?;
        crate::app::tray::update_sync_action(self.app, &snapshot);
        let _ = self.app.emit("sync-resumed", &snapshot);
        self.resume_recovered_work(&snapshot).await
    }

    async fn resume_recovered_work(
        &self,
        snapshot: &db::SyncSnapshot,
    ) -> Result<db::SyncSnapshot, String> {
        if snapshot.status != "RUNNING" || self.coordinator.lock.try_lock().is_err() {
            return Ok(snapshot.clone());
        }
        let has_pending = db::has_pending_sync_queue_items(self.pool)
            .await
            .map_err(|error| error.to_string())?;
        if !has_pending {
            let completed = db::complete_sync_job_if_finished(self.pool)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Sync queue is still processing.".to_string())?;
            crate::app::tray::update_sync_action(self.app, &completed);
            let _ = self.app.emit("sync-progress-snapshot", &completed);
            return Ok(completed);
        }
        let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
        let audit_context = SyncAuditContext::from_credentials(&credentials, "resume_sync");
        let client = create_authenticated_client(self.app, credentials)
            .ok_or("Invalid server configuration")?;
        crate::audit::audit_event(
            self.app,
            audit_context.event(
                "sync.resumed",
                crate::audit::Outcome::Info,
                crate::audit::Severity::Info,
                "Synchronization resumed",
                serde_json::json!({}),
            ),
        );
        let app = self.app.clone();
        let pool = self.pool.clone();
        let cleanup_pool = self.pool.clone();
        let coordinator = self.coordinator.clone();
        self.running.store(true, Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            let result = crate::sync::queue_runner::run_sync_pipeline(
                app.clone(),
                pool,
                client,
                ScanResult::default(),
                true,
                coordinator,
                audit_context,
            )
            .await;
            app.state::<crate::sync::SyncState>()
                .0
                .store(false, Ordering::SeqCst);
            if let Err(error) = result {
                emit_sync_error(&app, &error);
                if let Ok(snapshot) = db::set_sync_status(&cleanup_pool, "IDLE").await {
                    crate::app::tray::update_sync_action(&app, &snapshot);
                }
            }
        });
        Ok(snapshot.clone())
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, String> {
        self.lifecycle().status().await
    }
}

pub(crate) use crate::media::events::{add_event_paths, log_overlapping_folders};
pub(crate) use crate::media::model::{Asset, FileFailure, ScanResult};
pub(crate) use crate::media::scanner::{scan_folders_for_media, scan_watched_folders_for_media};

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

pub(crate) fn prepare_scan_for_queue(
    scan_result: &ScanResult,
    folders: &[db::WatchedFolder],
) -> (Vec<db::QueueUploadUnit>, Vec<(String, String)>) {
    let assets = scan_result
        .files
        .iter()
        .map(|asset| {
            let mut asset = asset.clone();
            let asset_path = std::path::PathBuf::from(&asset.path);
            asset.target_album_id = asset.target_album_id.or_else(|| {
                folders.iter().find_map(|f| {
                    let folder_path = std::path::PathBuf::from(&f.path);
                    if crate::media::paths::path_is_within(&asset_path, &folder_path) {
                        f.target_album_id.clone()
                    } else {
                        None
                    }
                })
            });
            asset
        })
        .collect::<Vec<_>>();
    let units = crate::media::model::upload_units_from_assets(assets)
        .into_iter()
        .map(|unit| db::QueueUploadUnit {
            image: queue_upload_asset(unit.image),
            live_photo_video: unit.live_photo_video.map(queue_upload_asset),
        })
        .collect();

    let failures = scan_result
        .failures
        .iter()
        .map(|failure| (failure.path.clone(), failure.error.clone()))
        .collect();

    (units, failures)
}

fn queue_upload_asset(asset: crate::media::model::Asset) -> db::QueueUploadAsset {
    db::QueueUploadAsset {
        path: asset.path,
        size: asset.size as i64,
        mtime: asset.mtime,
        target_album_id: asset.target_album_id,
    }
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
    let (units, failures) = prepare_scan_for_queue(&scan_result, &folders);
    match db::enqueue_upload_units(&pool, &units, &failures).await {
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

    // Automatic producers use the same durable lifecycle gate as manual work.
    // When another run owns it, the queued work remains durable and that run will
    // pick it up instead of starting a competing pipeline.
    let running = app.state::<crate::sync::SyncState>().0.clone();
    let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator.clone());
    let started = match lifecycle.start().await {
        Ok(snapshot) => snapshot,
        Err(_) => return,
    };
    crate::app::tray::update_sync_action(&app, &started);

    if let Ok(Some(credentials)) = auth::get_credentials() {
        let audit_context = SyncAuditContext::from_credentials(&credentials, source);
        if let Some(client) = create_authenticated_client(&app, credentials) {
            let app_for_runner = app.clone();
            let cleanup_pool = pool.clone();
            tauri::async_runtime::spawn(async move {
                let result = run_sync_pipeline(
                    app_for_runner.clone(),
                    pool,
                    client,
                    ScanResult::default(),
                    true,
                    coordinator,
                    audit_context,
                )
                .await;
                app_for_runner
                    .state::<crate::sync::SyncState>()
                    .0
                    .store(false, Ordering::SeqCst);
                if let Err(error) = result {
                    emit_sync_error(&app_for_runner, &error);
                    if let Ok(snapshot) = db::set_sync_status(&cleanup_pool, "IDLE").await {
                        crate::app::tray::update_sync_action(&app_for_runner, &snapshot);
                    }
                }
            });
        } else {
            let _ = lifecycle.abort().await;
        }
    } else {
        let _ = lifecycle.abort().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle_persists_pause_and_resume_without_a_desktop_runtime() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let running = std::sync::atomic::AtomicBool::new(false);
        let coordinator = std::sync::Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator.clone());

        assert_eq!(lifecycle.start().await.unwrap().status, "RUNNING");
        assert_eq!(lifecycle.pause().await.unwrap().status, "PAUSED");
        assert!(coordinator.paused.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(lifecycle.resume().await.unwrap().status, "RUNNING");
        assert!(!coordinator.paused.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(lifecycle.status().await.unwrap().status, "RUNNING");
    }

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

    #[tokio::test]
    async fn changing_a_watched_folders_target_album_after_enqueue_does_not_change_queued_work() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let scan = ScanResult {
            files: vec![Asset {
                path: "C:/photos/photo.jpg".to_string(),
                size: 100,
                mtime: 10,
                target_album_id: None,
                remote_id: None,
            }],
            ..Default::default()
        };
        let original_album = crate::immich::AlbumId::new("album-original");
        let changed_album = crate::immich::AlbumId::new("album-changed");
        let folders = [db::WatchedFolder {
            id: 1,
            path: "C:/photos".to_string(),
            recursive: true,
            target_album_id: Some(original_album),
            exclusion_patterns: vec![],
        }];
        let (units, failures) = prepare_scan_for_queue(&scan, &folders);
        db::enqueue_upload_units(&pool, &units, &failures)
            .await
            .unwrap();

        let changed_folders = [db::WatchedFolder {
            target_album_id: Some(changed_album),
            ..folders.into_iter().next().unwrap()
        }];
        let (units, failures) = prepare_scan_for_queue(&scan, &changed_folders);
        db::enqueue_upload_units(&pool, &units, &failures)
            .await
            .unwrap();

        let recovered = db::next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(
            recovered[0].image.target_album_id.as_deref(),
            Some("album-original")
        );
    }
}
