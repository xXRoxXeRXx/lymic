//! Tauri-facing sync service facade.

use crate::{
    app::events::{emit_sync_error, log_to_ui_error},
    audit::SyncAuditContext,
    auth, db,
    immich::ImmichClient,
    sync::{
        lifecycle::{LifecycleClaim, SynchronizationLifecycle},
        queue_runner::run_sync_pipeline,
        SyncCoordinator,
    },
};
use std::sync::Arc;
use tauri::{Emitter, Manager, Runtime};

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
            match result {
                Ok(summary) => Ok(Some(summary)),
                Err(error) => self.fail_run(source, &error).await,
            }
        } else {
            spawn_sync_runner(
                self.app.clone(),
                self.pool.clone(),
                client,
                source,
                self.coordinator.clone(),
                self.app.state::<crate::sync::SyncState>().0.clone(),
                audit_context,
            );
            Ok(None)
        }
    }

    async fn fail_run<T>(&self, source: SyncSource, error: &str) -> Result<T, String> {
        fail_sync_run(
            self.app,
            self.pool,
            self.running,
            self.coordinator.clone(),
            source,
            error,
        )
        .await;
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
        if snapshot.status != "RUNNING" {
            return Ok(snapshot.clone());
        }
        let lifecycle = self.lifecycle();
        if !matches!(lifecycle.start_or_join().await?, LifecycleClaim::Started(_)) {
            return lifecycle.status().await;
        }
        match lifecycle.complete().await {
            Ok(Some(completed)) => {
                crate::app::tray::update_sync_action(self.app, &completed);
                let _ = self.app.emit("sync-progress-snapshot", &completed);
                return Ok(completed);
            }
            Ok(None) => {}
            Err(error) => return self.fail_run(SyncSource::Recovery, &error).await,
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
        spawn_sync_runner(
            self.app.clone(),
            self.pool.clone(),
            client,
            SyncSource::Recovery,
            self.coordinator.clone(),
            self.app.state::<crate::sync::SyncState>().0.clone(),
            audit_context,
        );
        Ok(snapshot.clone())
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, String> {
        self.lifecycle().status().await
    }
}

fn spawn_sync_runner(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: Arc<ImmichClient>,
    source: SyncSource,
    coordinator: Arc<crate::sync::SyncCoordinatorInner>,
    running: Arc<std::sync::atomic::AtomicBool>,
    audit_context: SyncAuditContext,
) {
    tauri::async_runtime::spawn(async move {
        let result = run_sync_pipeline(
            app.clone(),
            pool.clone(),
            client,
            source,
            coordinator.clone(),
            running.clone(),
            audit_context,
        )
        .await;
        if let Err(error) = result {
            fail_sync_run(&app, &pool, &running, coordinator, source, &error).await;
            emit_sync_error(&app, &error);
        }
    });
}

async fn fail_sync_run<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &sqlx::SqlitePool,
    running: &std::sync::atomic::AtomicBool,
    coordinator: Arc<crate::sync::SyncCoordinatorInner>,
    source: SyncSource,
    error: &str,
) {
    if let Ok(snapshot) = SynchronizationLifecycle::new(pool, running, coordinator)
        .fail()
        .await
    {
        crate::app::tray::update_sync_action(app, &snapshot);
        let _ = app.emit("sync-progress-snapshot", &snapshot);
    }
    crate::audit::audit_event(app, sync_aborted_audit_event(source, error));
}

fn sync_aborted_audit_event(source: SyncSource, error: &str) -> crate::audit::AuditEvent {
    crate::audit::AuditEvent::new(
        crate::audit::AuditEvent::operation_id(),
        "sync.aborted",
        crate::audit::Outcome::Failure,
        crate::audit::Severity::Error,
        None,
        source.audit_label(),
        "Synchronization aborted",
        serde_json::json!({ "failure_reason": crate::audit::audit_safe_error(error, &[]) }),
    )
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
pub(crate) use crate::media::scanner::scan_watched_folders_for_media;

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
    use std::sync::atomic::Ordering;
    use tauri::Listener;

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

    #[tokio::test]
    async fn lifecycle_keeps_ownership_when_work_arrives_before_completion() {
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
        let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(_)
        ));
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "arrived.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        assert!(lifecycle.complete().await.unwrap().is_none());
        assert!(running.load(Ordering::SeqCst));
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Joined
        ));
    }

    #[tokio::test]
    async fn failed_run_keeps_pending_work_when_new_work_is_admitted() {
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
        let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);
        let first = db::QueueAsset {
            path: "first.jpg",
            size: 1,
            mtime: 1,
            target_album_id: None,
        };
        db::enqueue_sync_assets(&pool, &[first], &[]).await.unwrap();
        lifecycle.start_or_join().await.unwrap();
        lifecycle.fail().await.unwrap();
        let second = db::QueueAsset {
            path: "second.jpg",
            size: 1,
            mtime: 1,
            target_album_id: None,
        };
        db::enqueue_sync_assets(&pool, &[second], &[])
            .await
            .unwrap();
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(_)
        ));
        assert_eq!(
            db::next_queued_upload_units(&pool, 10).await.unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn restart_pauses_running_work_until_resumed() {
        let database_path =
            std::env::temp_dir().join(format!("lymic-recovery-{}.db", uuid::Uuid::new_v4()));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let asset = db::QueueAsset {
            path: "pending.jpg",
            size: 1,
            mtime: 1,
            target_album_id: None,
        };
        db::enqueue_sync_assets(&pool, &[asset], &[]).await.unwrap();
        pool.close().await;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        assert_eq!(
            SynchronizationLifecycle::recover(&pool)
                .await
                .unwrap()
                .status,
            "PAUSED"
        );
        let running = std::sync::atomic::AtomicBool::new(false);
        let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(true));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Paused
        ));
        lifecycle.resume().await.unwrap();
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(_)
        ));
        assert_eq!(
            db::next_queued_upload_units(&pool, 10).await.unwrap().len(),
            1
        );
        pool.close().await;
        std::fs::remove_file(database_path).unwrap();
    }

    #[tokio::test]
    async fn admission_racing_completion_always_has_a_runner() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(4)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=shared"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let running = std::sync::atomic::AtomicBool::new(false);
        let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);
        lifecycle.start_or_join().await.unwrap();
        let (completion, claim) = tokio::join!(lifecycle.complete(), async {
            db::enqueue_sync_assets(
                &pool,
                &[db::QueueAsset {
                    path: "during-completion.jpg",
                    size: 1,
                    mtime: 1,
                    target_album_id: None,
                }],
                &[],
            )
            .await
            .unwrap();
            lifecycle.start_or_join().await.unwrap()
        });
        match completion.unwrap() {
            Some(_) => assert!(matches!(claim, LifecycleClaim::Started(_))),
            None => assert!(matches!(claim, LifecycleClaim::Joined)),
        }
        assert!(running.load(Ordering::SeqCst));
        assert_eq!(
            db::next_queued_upload_units(&pool, 10).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn retry_after_completion_preserves_remote_asset_for_album_assignment() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let album = crate::immich::AlbumId::new("album-1");
        let asset = db::QueueAsset {
            path: "photo.jpg",
            size: 1,
            mtime: 1,
            target_album_id: Some(&album),
        };
        db::enqueue_sync_assets(&pool, &[asset.clone()], &[])
            .await
            .unwrap();
        db::update_sync_queue_remote_id(
            &pool,
            "photo.jpg",
            &crate::immich::RemoteAssetId::new("remote-1"),
        )
        .await
        .unwrap();
        db::finalize_queued_block(&pool, &[("photo.jpg".into(), "FAILED".into())])
            .await
            .unwrap();
        let running = std::sync::atomic::AtomicBool::new(true);
        let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
        let lifecycle = SynchronizationLifecycle::new(&pool, &running, coordinator);
        assert!(lifecycle.complete().await.unwrap().is_some());
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "unrelated.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        db::finalize_queued_block(&pool, &[("unrelated.jpg".into(), "SYNCED".into())])
            .await
            .unwrap();
        db::complete_sync_job_if_finished(&pool).await.unwrap();
        db::enqueue_sync_assets(&pool, &[asset], &[]).await.unwrap();
        let queued = db::next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(
            queued[0].image.remote_id.as_ref().map(|id| id.as_str()),
            Some("remote-1")
        );
        assert_eq!(
            queued[0]
                .image
                .target_album_id
                .as_ref()
                .map(|id| id.as_str()),
            Some("album-1")
        );
    }

    #[tokio::test]
    async fn changed_file_does_not_reuse_remote_asset_while_other_work_is_pending() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let original = db::QueueAsset {
            path: "photo.jpg",
            size: 1,
            mtime: 1,
            target_album_id: None,
        };
        let other = db::QueueAsset {
            path: "other.jpg",
            size: 1,
            mtime: 1,
            target_album_id: None,
        };
        db::enqueue_sync_assets(&pool, &[original, other], &[])
            .await
            .unwrap();
        db::update_sync_queue_remote_id(
            &pool,
            "photo.jpg",
            &crate::immich::RemoteAssetId::new("old-id"),
        )
        .await
        .unwrap();
        db::finalize_queued_block(&pool, &[("photo.jpg".into(), "FAILED".into())])
            .await
            .unwrap();
        let changed = db::QueueAsset {
            path: "photo.jpg",
            size: 2,
            mtime: 2,
            target_album_id: None,
        };
        db::enqueue_sync_assets(&pool, &[changed], &[])
            .await
            .unwrap();
        let queued = db::next_queued_upload_units(&pool, 10).await.unwrap();
        assert!(queued
            .iter()
            .find(|unit| unit.image.local_path == "photo.jpg")
            .unwrap()
            .image
            .remote_id
            .is_none());
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

    #[test]
    fn manual_and_background_failures_use_the_same_aborted_sync_audit_event() {
        let manual = serde_json::to_value(sync_aborted_audit_event(
            SyncSource::Manual,
            "connection lost",
        ))
        .unwrap();
        let background = serde_json::to_value(sync_aborted_audit_event(
            SyncSource::Watcher,
            "connection lost",
        ))
        .unwrap();

        assert_eq!(manual["event_type"], "sync.aborted");
        assert_eq!(background["event_type"], "sync.aborted");
        assert_eq!(manual["outcome"], "failure");
        assert_eq!(background["outcome"], "failure");
        assert_eq!(manual["source"], "manual_sync");
        assert_eq!(background["source"], "watcher_sync");
        assert_eq!(
            manual["details"]["failure_reason"],
            serde_json::Value::String("connection lost".to_string())
        );
        assert_eq!(manual["details"], background["details"]);
    }

    #[tokio::test]
    async fn shared_failure_cleanup_persists_failed_state_and_emits_a_snapshot_for_manual_and_background_sources(
    ) {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let app = tauri::test::mock_app();
        let audit_logger =
            std::sync::Arc::new(crate::audit::AuditLogger::initialize(app.handle()).unwrap());
        app.manage(audit_logger.clone());
        let snapshots = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_snapshots = snapshots.clone();
        app.handle().listen("sync-progress-snapshot", move |event| {
            captured_snapshots
                .lock()
                .unwrap()
                .push(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap());
        });
        let running = std::sync::atomic::AtomicBool::new(true);
        let coordinator = std::sync::Arc::new(crate::sync::SyncCoordinatorInner::new(false));

        for source in [SyncSource::Manual, SyncSource::Watcher] {
            running.store(true, Ordering::SeqCst);
            fail_sync_run(
                app.handle(),
                &pool,
                &running,
                coordinator.clone(),
                source,
                "connection lost",
            )
            .await;

            assert_eq!(db::get_sync_snapshot(&pool).await.unwrap().status, "FAILED");
            assert!(!running.load(Ordering::SeqCst));
        }

        let snapshots = snapshots.lock().unwrap();
        assert_eq!(snapshots.len(), 2);
        for snapshot in snapshots.iter() {
            assert_eq!(snapshot["status"], "FAILED");
        }

        audit_logger.flush().unwrap();
        let audit_log = std::fs::read_to_string(
            app.handle()
                .path()
                .app_data_dir()
                .unwrap()
                .join("logs")
                .join("audit-0.jsonl"),
        )
        .unwrap();
        let aborted_sources = audit_log
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["event_type"] == "sync.aborted")
            .map(|event| event["source"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(aborted_sources.contains(&"manual_sync".to_string()));
        assert!(aborted_sources.contains(&"watcher_sync".to_string()));
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
