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

/// Identifies the producer submitting Synchronization work.
///
/// This keeps audit/UI behaviour explicit at the admission boundary instead of
/// allowing producers to pass behaviour-changing string literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncSource {
    Manual,
    RetryFailed,
    Watcher,
    Startup,
    DeferredFolder,
    FolderAdded,
    ExclusionPatternsChanged,
    RemovableMediaReconnect,
    Recovery,
}

impl SyncSource {
    pub(crate) const fn audit_label(self) -> &'static str {
        match self {
            Self::Manual => "manual_sync",
            Self::RetryFailed => "retry_failed_syncs",
            Self::Watcher => "watcher_sync",
            Self::Startup => "startup_sync",
            Self::DeferredFolder => "deferred_folder",
            Self::FolderAdded => "folder_add",
            Self::ExclusionPatternsChanged => "exclusion_patterns_changed",
            Self::RemovableMediaReconnect => "removable_media_reconnect",
            Self::Recovery => "resume_sync",
        }
    }

    pub(crate) const fn is_manual(self) -> bool {
        matches!(self, Self::Manual | Self::RetryFailed)
    }
}

#[derive(Debug, Clone)]
enum LifecycleClaim {
    Started(db::SyncSnapshot),
    Joined,
    Paused,
}

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

    async fn start_or_join(&self) -> Result<LifecycleClaim, String> {
        let current = self.status().await?;
        if current.status == "PAUSED" {
            return Ok(LifecycleClaim::Paused);
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(LifecycleClaim::Joined);
        }

        match db::set_sync_status(self.pool, "RUNNING").await {
            Ok(snapshot) => Ok(LifecycleClaim::Started(snapshot)),
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                Err(error.to_string())
            }
        }
    }

    pub(crate) fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub(crate) async fn fail(&self) -> Result<db::SyncSnapshot, String> {
        self.finish();
        db::set_sync_status(self.pool, "FAILED")
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

    /// Admit producer output into the durable queue before any runner observes it.
    /// This is the shared boundary for manual scans, watcher batches, remounts,
    /// and recovery; producers do not choose queue ordering or runner ownership.
    pub(crate) async fn admit(&self, scan_result: ScanResult) -> Result<db::SyncSnapshot, String> {
        log_overlapping_folders(self.app, &scan_result);
        let snapshot = admit_scan_result(self.pool, &scan_result).await?;
        crate::app::tray::update_sync_action(self.app, &snapshot);
        let _ = self.app.emit("sync-progress-snapshot", &snapshot);
        Ok(snapshot)
    }

    pub(crate) async fn start(&self) -> Result<crate::sync::SyncSummary, String> {
        let folders = db::get_folders(self.pool)
            .await
            .map_err(|error| error.to_string())?;
        ensure_sync_has_folders(&folders)?;
        crate::app::events::log_to_ui_key(
            self.app,
            "INFO",
            "Starting manual synchronization...",
            "log_starting_sync",
        );

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
        Ok(self
            .admit_and_run(scan_result, SyncSource::Manual)
            .await?
            .unwrap_or_default())
    }

    pub(crate) async fn retry(&self) -> Result<crate::sync::SyncSummary, String> {
        let failed_paths = db::get_failed_syncs(self.pool)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.local_path)
            .collect();
        let folders = db::get_folders(self.pool)
            .await
            .map_err(|error| error.to_string())?;
        let scan_result = crate::sync::prepare_failed_sync_retry(failed_paths, folders).await?;
        db::discard_failed_sync_paths(self.pool, &scan_result.discarded_paths)
            .await
            .map_err(|error| error.to_string())?;
        Ok(self
            .admit_and_run(scan_result, SyncSource::RetryFailed)
            .await?
            .unwrap_or_default())
    }

    /// The sole producer boundary: persist work, claim or join the lifecycle,
    /// construct authenticated execution, and retain a durable failure state.
    /// Automatic producers return once their work is admitted; manual producers
    /// await the runner they successfully claimed.
    pub(crate) async fn admit_and_run(
        &self,
        scan_result: ScanResult,
        source: SyncSource,
    ) -> Result<Option<crate::sync::SyncSummary>, String> {
        if scan_result.files.is_empty() && scan_result.failures.is_empty() {
            return Ok(None);
        }
        self.admit(scan_result).await?;

        let lifecycle = self.lifecycle();
        let claim = lifecycle.start_or_join().await?;
        let LifecycleClaim::Started(started) = claim else {
            return Ok(None);
        };
        crate::app::tray::update_sync_action(self.app, &started);

        let credentials = match auth::get_credentials() {
            Ok(Some(credentials)) => credentials,
            Ok(None) => return self.fail_run(source, "Not logged in").await,
            Err(error) => return self.fail_run(source, &error.to_string()).await,
        };
        let audit_context = SyncAuditContext::from_credentials(&credentials, source.audit_label());
        let Some(client) = create_authenticated_client(self.app, credentials) else {
            return self.fail_run(source, "Invalid server configuration").await;
        };

        if source.is_manual() {
            let result = run_sync_pipeline(
                self.app.clone(),
                self.pool.clone(),
                client,
                source,
                self.coordinator.clone(),
                self.app.state::<crate::sync::SyncState>().0.clone(),
                audit_context,
            )
            .await;
            lifecycle.finish();
            match result {
                Ok(summary) => Ok(Some(summary)),
                Err(error) => self.fail_run(source, &error).await,
            }
        } else {
            let app = self.app.clone();
            let pool = self.pool.clone();
            let coordinator = self.coordinator.clone();
            let running = self.app.state::<crate::sync::SyncState>().0.clone();
            tauri::async_runtime::spawn(async move {
                let result = run_sync_pipeline(
                    app.clone(),
                    pool.clone(),
                    client,
                    source,
                    coordinator,
                    running,
                    audit_context,
                )
                .await;
                app.state::<crate::sync::SyncState>()
                    .0
                    .store(false, Ordering::SeqCst);
                if let Err(error) = result {
                    emit_sync_error(&app, &error);
                    if let Ok(snapshot) = db::set_sync_status(&pool, "FAILED").await {
                        crate::app::tray::update_sync_action(&app, &snapshot);
                    }
                }
            });
            Ok(None)
        }
    }

    async fn fail_run<T>(&self, source: SyncSource, error: &str) -> Result<T, String> {
        if let Ok(snapshot) = self.lifecycle().fail().await {
            crate::app::tray::update_sync_action(self.app, &snapshot);
            let _ = self.app.emit("sync-progress-snapshot", &snapshot);
        }
        crate::audit::audit_event(
            self.app,
            crate::audit::AuditEvent::new(
                crate::audit::AuditEvent::operation_id(),
                "sync.aborted",
                crate::audit::Outcome::Failure,
                crate::audit::Severity::Error,
                None,
                source.audit_label(),
                "Synchronization aborted",
                serde_json::json!({ "failure_reason": crate::audit::audit_safe_error(error, &[]) }),
            ),
        );
        Err(error.to_string())
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
        let credentials = match auth::get_credentials() {
            Ok(Some(credentials)) => credentials,
            Ok(None) => return self.fail_run(SyncSource::Recovery, "Not logged in").await,
            Err(error) => {
                return self
                    .fail_run(SyncSource::Recovery, &error.to_string())
                    .await
            }
        };
        let audit_context =
            SyncAuditContext::from_credentials(&credentials, SyncSource::Recovery.audit_label());
        let Some(client) = create_authenticated_client(self.app, credentials) else {
            return self
                .fail_run(SyncSource::Recovery, "Invalid server configuration")
                .await;
        };
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
        let running = self.app.state::<crate::sync::SyncState>().0.clone();
        self.running.store(true, Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            let result = crate::sync::queue_runner::run_sync_pipeline(
                app.clone(),
                pool,
                client,
                SyncSource::Recovery,
                coordinator,
                running,
                audit_context,
            )
            .await;
            app.state::<crate::sync::SyncState>()
                .0
                .store(false, Ordering::SeqCst);
            if let Err(error) = result {
                emit_sync_error(&app, &error);
                if let Ok(snapshot) = db::set_sync_status(&cleanup_pool, "FAILED").await {
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

/// Queue a producer result while copying its Watched Folder's current Target
/// Album into each Upload Unit. Kept free of Tauri so the durable admission
/// contract is directly testable.
pub(crate) async fn admit_scan_result(
    pool: &sqlx::SqlitePool,
    scan_result: &ScanResult,
) -> Result<db::SyncSnapshot, String> {
    let folders = db::get_folders(pool)
        .await
        .map_err(|error| error.to_string())?;
    let (units, failures) = prepare_scan_for_queue(scan_result, &folders);
    db::enqueue_upload_units(pool, &units, &failures)
        .await
        .map_err(|error| format!("Could not persist sync queue: {error}"))
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
    let folders = folders
        .iter()
        .map(|folder| {
            (
                crate::media::paths::normalize_path_lexical(std::path::Path::new(&folder.path)),
                folder.target_album_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let assets = scan_result
        .files
        .iter()
        .map(|asset| {
            let mut asset = asset.clone();
            let asset_path =
                crate::media::paths::normalize_path_lexical(std::path::Path::new(&asset.path));
            asset.target_album_id = asset.target_album_id.or_else(|| {
                folders.iter().find_map(|(folder_path, target_album_id)| {
                    if crate::media::paths::path_is_within_lexical(&asset_path, folder_path) {
                        target_album_id.clone()
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

/// Producer-facing Synchronization admission-and-run seam.
pub(crate) async fn admit_and_run(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    scan_result: ScanResult,
    source: SyncSource,
) {
    let coordinator = app.state::<SyncCoordinator>().0.clone();
    let running = app.state::<crate::sync::SyncState>().0.clone();
    let synchronization = Synchronization::new(&app, &pool, &running, &coordinator);
    match synchronization.admit_and_run(scan_result, source).await {
        Ok(_) => {}
        Err(error) => {
            if source.is_manual() {
                emit_sync_error(&app, &format!("Synchronization admission failed: {error}"));
            }
        }
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

        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(snapshot) if snapshot.status == "RUNNING"
        ));
        assert_eq!(lifecycle.pause().await.unwrap().status, "PAUSED");
        assert!(coordinator.paused.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(lifecycle.resume().await.unwrap().status, "RUNNING");
        assert!(!coordinator.paused.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(lifecycle.status().await.unwrap().status, "RUNNING");
    }

    #[tokio::test]
    async fn concurrent_manual_admissions_join_one_lifecycle_without_losing_either_result() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        db::add_folder(&pool, "C:/photos", None).await.unwrap();
        let running = std::sync::atomic::AtomicBool::new(false);
        let coordinator = std::sync::Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);

        let result_for = |path: &str| ScanResult {
            files: vec![Asset {
                path: path.to_string(),
                size: 100,
                mtime: 10,
                target_album_id: None,
                remote_id: None,
            }],
            ..Default::default()
        };
        admit_scan_result(&pool, &result_for("C:/photos/first.jpg"))
            .await
            .unwrap();
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(_)
        ));

        admit_scan_result(&pool, &result_for("C:/photos/second.jpg"))
            .await
            .unwrap();
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Joined
        ));
        let queued = db::next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(queued.len(), 2);
    }

    #[tokio::test]
    async fn lifecycle_failure_is_durable_without_discarding_queued_work() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "C:/photos/photo.jpg",
                size: 100,
                mtime: 10,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        let running = std::sync::atomic::AtomicBool::new(true);
        let coordinator = std::sync::Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);

        assert_eq!(lifecycle.fail().await.unwrap().status, "FAILED");
        assert!(db::has_pending_sync_queue_items(&pool).await.unwrap());
    }

    #[test]
    fn producer_sources_keep_audit_labels_and_manual_execution_rules_explicit() {
        assert_eq!(SyncSource::Manual.audit_label(), "manual_sync");
        assert!(SyncSource::Manual.is_manual());
        assert_eq!(SyncSource::Watcher.audit_label(), "watcher_sync");
        assert!(!SyncSource::Watcher.is_manual());
        assert_eq!(
            SyncSource::RemovableMediaReconnect.audit_label(),
            "removable_media_reconnect"
        );
        assert_eq!(SyncSource::Recovery.audit_label(), "resume_sync");
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
        db::add_folder(&pool, "C:/photos", Some(&original_album))
            .await
            .unwrap();
        admit_scan_result(&pool, &scan).await.unwrap();

        // A later Watched Folder change must not redirect an Upload Unit already
        // admitted to the durable queue.
        sqlx::query(
            "UPDATE watched_folders SET target_album_id = 'album-changed' WHERE path = 'C:/photos'",
        )
        .execute(&pool)
        .await
        .unwrap();
        admit_scan_result(&pool, &scan).await.unwrap();

        let recovered = db::next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(
            recovered[0].image.target_album_id.as_deref(),
            Some("album-original")
        );
    }
}
