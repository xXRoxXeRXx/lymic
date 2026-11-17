//! Tauri-facing sync service facade.

use crate::{
    app::events::log_to_ui_error,
    audit::SyncAuditContext,
    auth, db,
    immich::{ImmichClient, ImmichSession},
    media::{
        events::log_overlapping_folders, model::ScanResult, scanner::scan_watched_folders_for_media,
    },
    sync::{
        lifecycle::{LifecycleClaim, SynchronizationLifecycle},
        queue_runner::run_sync_pipeline,
        reporter::{DesktopRunReporter, SyncRunReporter},
        SyncRuntime,
    },
};
use std::sync::Arc;
use tauri::{Manager, Runtime};

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
    runtime: &'a SyncRuntime,
}

impl<'a> Synchronization<'a> {
    pub(crate) fn new(app: &'a tauri::AppHandle, runtime: &'a SyncRuntime) -> Self {
        Self { app, runtime }
    }

    pub(crate) fn for_tauri(app: &'a tauri::AppHandle) -> Self {
        Self::new(app, app.state::<SyncRuntime>().inner())
    }

    fn pool(&self) -> &sqlx::SqlitePool {
        self.runtime.pool()
    }

    fn lifecycle(&self) -> SynchronizationLifecycle<'_> {
        SynchronizationLifecycle::new(self.runtime)
    }

    fn reporter(&self) -> DesktopRunReporter {
        DesktopRunReporter::for_service(self.app.clone())
    }

    pub(crate) async fn start(&self) -> Result<crate::sync::SyncSummary, String> {
        let folders = db::get_folders(self.pool())
            .await
            .map_err(|error| error.to_string())?;
        prepare_manual_sync(self.app, &folders)?;
        Ok(self
            .scan_and_admit(folders, SyncSource::Manual)
            .await?
            .unwrap_or_default())
    }

    /// The scan-and-admit entry point for producers that have Watched Folders to
    /// scan: logs each folder, scans for Media Assets, logs scan failures, and
    /// admits the result into Synchronization.
    pub(crate) async fn scan_and_admit(
        &self,
        folders: Vec<db::WatchedFolder>,
        source: SyncSource,
    ) -> Result<Option<crate::sync::SyncSummary>, String> {
        let scan_result = scan_folders_with_logging(self.app, folders).await?;
        self.admit_and_run(scan_result, source).await
    }

    pub(crate) async fn retry(&self) -> Result<crate::sync::SyncSummary, String> {
        let failed_paths = db::get_failed_syncs(self.pool())
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.local_path)
            .collect();
        let folders = db::get_folders(self.pool())
            .await
            .map_err(|error| error.to_string())?;
        let scan_result = crate::sync::prepare_failed_sync_retry(failed_paths, folders).await?;
        self.lifecycle()
            .discard_failed(&scan_result.discarded_paths)
            .await?;
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
        log_overlapping_folders(self.app, &scan_result);
        let lifecycle = self.lifecycle();
        let reporter = self.reporter();
        let (admitted, claim) = lifecycle.admit_and_claim(&scan_result).await?;
        if let Some(snapshot) = admitted {
            reporter.progressed(&snapshot);
        }
        let LifecycleClaim::Started(started) = claim else {
            return Ok(None);
        };
        reporter.claimed(&started);

        let session = self.open_session(source).await?;
        let audit_context =
            SyncAuditContext::from_credentials(session.credentials(), source.audit_label());
        let client = session.client();

        if source.is_manual() {
            let result = run_sync_pipeline(
                self.app.clone(),
                self.runtime.clone(),
                client,
                source,
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
                self.runtime.clone(),
                client,
                source,
                audit_context,
            );
            Ok(None)
        }
    }

    /// Obtains the authenticated Immich session for a run, failing the run durably
    /// when there is no login or the stored server configuration is invalid.
    async fn open_session(&self, source: SyncSource) -> Result<ImmichSession, String> {
        let credentials = match auth::get_credentials() {
            Ok(Some(credentials)) => credentials,
            Ok(None) => return self.fail_run(source, "Not logged in").await,
            Err(error) => return self.fail_run(source, &error).await,
        };
        match ImmichSession::from_credentials(credentials) {
            Ok(session) => Ok(session),
            Err(error) => {
                log_to_ui_error(
                    self.app,
                    "ERROR",
                    &format!("Invalid server configuration: {}", error),
                    "log_invalid_server_config",
                    &error,
                );
                self.fail_run(source, "Invalid server configuration").await
            }
        }
    }

    async fn fail_run<T>(&self, source: SyncSource, error: &str) -> Result<T, String> {
        fail_sync_run(self.app, self.runtime, source, error).await;
        Err(error.to_string())
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, String> {
        let snapshot = self.lifecycle().pause().await?;
        self.reporter().paused(&snapshot);
        Ok(snapshot)
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, String> {
        let previous = self.lifecycle().status().await?;
        if previous.status != db::SyncJobStatus::Paused {
            return Ok(previous);
        }
        let snapshot = self.lifecycle().resume().await?;
        self.reporter().resumed(&snapshot);
        self.resume_recovered_work(&snapshot).await
    }

    async fn resume_recovered_work(
        &self,
        snapshot: &db::SyncSnapshot,
    ) -> Result<db::SyncSnapshot, String> {
        if snapshot.status != db::SyncJobStatus::Running {
            return Ok(snapshot.clone());
        }
        let lifecycle = self.lifecycle();
        if !matches!(lifecycle.start_or_join().await?, LifecycleClaim::Started(_)) {
            return lifecycle.status().await.map_err(Into::into);
        }
        match lifecycle.complete().await {
            Ok(Some(completed)) => {
                self.reporter().progressed(&completed);
                return Ok(completed);
            }
            Ok(None) => {}
            Err(error) => {
                return self
                    .fail_run(SyncSource::Recovery, &error.to_string())
                    .await
            }
        }
        let session = self.open_session(SyncSource::Recovery).await?;
        let audit_context = SyncAuditContext::from_credentials(
            session.credentials(),
            SyncSource::Recovery.audit_label(),
        );
        let client = session.client();
        self.reporter().resumed_audit(&audit_context);
        spawn_sync_runner(
            self.app.clone(),
            self.runtime.clone(),
            client,
            SyncSource::Recovery,
            audit_context,
        );
        Ok(snapshot.clone())
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, String> {
        self.lifecycle().status().await.map_err(Into::into)
    }
}

fn spawn_sync_runner(
    app: tauri::AppHandle,
    runtime: SyncRuntime,
    client: Arc<ImmichClient>,
    source: SyncSource,
    audit_context: SyncAuditContext,
) {
    tauri::async_runtime::spawn(async move {
        let result =
            run_sync_pipeline(app.clone(), runtime.clone(), client, source, audit_context).await;
        if let Err(error) = result {
            fail_sync_run(&app, &runtime, source, &error).await;
            DesktopRunReporter::for_service(app).error(&error);
        }
    });
}

async fn fail_sync_run<R: Runtime>(
    app: &tauri::AppHandle<R>,
    runtime: &SyncRuntime,
    source: SyncSource,
    error: &str,
) {
    let snapshot = SynchronizationLifecycle::new(runtime).fail().await.ok();
    DesktopRunReporter::for_service(app.clone()).failed(source, snapshot.as_ref(), error);
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

pub(crate) fn ensure_sync_has_folders(folders: &[db::WatchedFolder]) -> Result<(), String> {
    if folders.is_empty() {
        return Err("Add at least one folder before starting a sync.".to_string());
    }
    Ok(())
}

fn prepare_manual_sync<R: Runtime>(
    app: &tauri::AppHandle<R>,
    folders: &[db::WatchedFolder],
) -> Result<(), String> {
    ensure_sync_has_folders(folders)?;
    crate::app::events::log_to_ui_key(
        app,
        "INFO",
        "Starting manual synchronization...",
        "log_starting_sync",
    );
    Ok(())
}

async fn scan_folders_with_logging<R: Runtime>(
    app: &tauri::AppHandle<R>,
    folders: Vec<db::WatchedFolder>,
) -> Result<ScanResult, String> {
    scan_folders_with_logging_using(app, folders, scan_watched_folders_for_media).await
}

async fn scan_folders_with_logging_using<R, Scan, ScanFuture>(
    app: &tauri::AppHandle<R>,
    folders: Vec<db::WatchedFolder>,
    scan: Scan,
) -> Result<ScanResult, String>
where
    R: Runtime,
    Scan: FnOnce(Vec<db::WatchedFolder>) -> ScanFuture,
    ScanFuture: std::future::Future<Output = Result<ScanResult, String>>,
{
    for folder in &folders {
        crate::app::events::log_to_ui_folder(
            app,
            "INFO",
            &format!("Scanning folder: {}", folder.path),
            "log_scanning_folder",
            &folder.path,
        );
    }
    match scan(folders).await {
        Ok(scan_result) => Ok(scan_result),
        Err(error) => {
            log_to_ui_error(
                app,
                "ERROR",
                &format!("Folder scanning failed: {error}"),
                "log_scan_folders_failed",
                &error,
            );
            Err(error)
        }
    }
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
    scan_result: ScanResult,
    source: SyncSource,
) {
    let synchronization = Synchronization::for_tauri(&app);
    match synchronization.admit_and_run(scan_result, source).await {
        Ok(_) => {}
        Err(error) => {
            if source.is_manual() {
                DesktopRunReporter::for_service(app)
                    .error(&format!("Synchronization admission failed: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::Asset;
    use tauri::Listener;

    async fn test_pool() -> sqlx::SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
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
    async fn lifecycle_persists_pause_and_resume_without_a_desktop_runtime() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool, false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);

        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Started(snapshot) if snapshot.status == db::SyncJobStatus::Running
        ));
        assert_eq!(
            lifecycle.pause().await.unwrap().status,
            db::SyncJobStatus::Paused
        );
        assert!(!runtime.can_start_work());
        assert_eq!(
            lifecycle.resume().await.unwrap().status,
            db::SyncJobStatus::Running
        );
        assert!(runtime.can_start_work());
        assert_eq!(
            lifecycle.status().await.unwrap().status,
            db::SyncJobStatus::Running
        );
    }

    #[tokio::test]
    async fn concurrent_manual_admissions_join_one_lifecycle_without_losing_either_result() {
        let pool = test_pool().await;
        db::add_folder(&pool, "C:/photos", None).await.unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);

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
        let pool = test_pool().await;
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
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.try_claim_run();
        let lifecycle = SynchronizationLifecycle::new(&runtime);

        assert_eq!(
            lifecycle.fail().await.unwrap().status,
            db::SyncJobStatus::Failed
        );
        assert!(db::has_pending_sync_queue_items(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn lifecycle_keeps_ownership_when_work_arrives_before_completion() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
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
        assert!(runtime.is_running());
        assert!(matches!(
            lifecycle.start_or_join().await.unwrap(),
            LifecycleClaim::Joined
        ));
    }

    #[tokio::test]
    async fn failed_run_keeps_pending_work_when_new_work_is_admitted() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
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
        let (runtime, recovered) = SyncRuntime::recover(pool.clone()).await.unwrap();
        assert_eq!(recovered.status, db::SyncJobStatus::Paused);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
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
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
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
        assert!(runtime.is_running());
        assert_eq!(
            db::next_queued_upload_units(&pool, 10).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn retry_after_completion_preserves_remote_asset_for_album_assignment() {
        let pool = test_pool().await;
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
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.try_claim_run();
        let lifecycle = SynchronizationLifecycle::new(&runtime);
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
    async fn changed_media_asset_does_not_reuse_remote_asset_while_other_work_is_pending() {
        let pool = test_pool().await;
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

    #[tokio::test]
    async fn runner_completes_after_its_last_folder_is_removed() {
        let pool = test_pool().await;
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "C:/photos/asset.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
        lifecycle.start_or_join().await.unwrap();
        db::discard_sync_data_in_folder(&pool, "C:/photos")
            .await
            .unwrap();
        assert_eq!(
            lifecycle
                .complete()
                .await
                .unwrap()
                .expect("runner must finish after its work is discarded")
                .status,
            db::SyncJobStatus::Idle
        );
        assert!(!runtime.is_running());
    }

    #[tokio::test]
    async fn admission_during_completion_preserves_previous_results() {
        let pool = test_pool().await;
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "first.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
        lifecycle.start_or_join().await.unwrap();
        db::finalize_queued_block(&pool, &[("first.jpg".into(), "SYNCED".into())])
            .await
            .unwrap();
        let snapshot = db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "second.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        assert_eq!((snapshot.total, snapshot.succeeded), (2, 1));
    }

    #[tokio::test]
    async fn restart_pauses_pending_work_even_if_no_runner_was_claimed() {
        let pool = test_pool().await;
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "recovered.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        db::set_sync_status(&pool, db::SyncJobStatus::Idle)
            .await
            .unwrap();
        assert_eq!(
            SynchronizationLifecycle::recover(&pool)
                .await
                .unwrap()
                .status,
            db::SyncJobStatus::Paused
        );
    }

    #[tokio::test]
    async fn empty_remount_scan_claims_restored_pending_work() {
        let pool = test_pool().await;
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "C:/photos/asset.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();
        db::finalize_queued_block(&pool, &[("C:/photos/asset.jpg".into(), "FAILED".into())])
            .await
            .unwrap();
        db::reset_failed_queue_items_for_folder(&pool, "C:/photos")
            .await
            .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        let lifecycle = SynchronizationLifecycle::new(&runtime);
        lifecycle.pause().await.unwrap();
        let (_, claim) = lifecycle
            .admit_and_claim(&ScanResult::default())
            .await
            .unwrap();
        assert!(matches!(claim, LifecycleClaim::Paused));
        assert!(!runtime.is_running());
        lifecycle.resume().await.unwrap();
        let (_, claim) = lifecycle
            .admit_and_claim(&ScanResult::default())
            .await
            .unwrap();
        assert!(matches!(claim, LifecycleClaim::Started(_)));
        assert!(runtime.is_running());
        assert_eq!(
            db::next_queued_upload_units(&pool, 10).await.unwrap().len(),
            1
        );
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
        let manual = serde_json::to_value(crate::sync::reporter::sync_aborted_audit_event(
            SyncSource::Manual,
            "connection lost",
        ))
        .unwrap();
        let background = serde_json::to_value(crate::sync::reporter::sync_aborted_audit_event(
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
        let pool = test_pool().await;
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
        let runtime = SyncRuntime::new(pool.clone(), false);

        for source in [SyncSource::Manual, SyncSource::Watcher] {
            runtime.try_claim_run();
            fail_sync_run(app.handle(), &runtime, source, "connection lost").await;

            assert_eq!(
                db::get_sync_snapshot(&pool).await.unwrap().status,
                db::SyncJobStatus::Failed
            );
            assert!(!runtime.is_running());
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

    #[tokio::test]
    async fn changing_a_watched_folders_target_album_after_enqueue_does_not_change_queued_work() {
        let pool = test_pool().await;

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

    #[tokio::test]
    async fn scan_folders_with_logging_emits_folder_logs_and_returns_discovered_assets() {
        let app = tauri::test::mock_app();
        let logs = crate::app::events::CapturedUiLogs::listen(app.handle());
        let root = std::env::temp_dir().join(format!("lymic-scan-admit-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("keep.jpg"), b"keep").unwrap();
        std::fs::write(first.join("skip.jpg"), b"skip").unwrap();
        std::fs::write(second.join("second.jpg"), b"second").unwrap();

        let folders = vec![
            db::WatchedFolder {
                id: 1,
                path: first.to_string_lossy().to_string(),
                recursive: true,
                target_album_id: None,
                exclusion_patterns: vec!["skip.jpg".to_string()],
            },
            db::WatchedFolder {
                id: 2,
                path: second.to_string_lossy().to_string(),
                recursive: true,
                target_album_id: None,
                exclusion_patterns: vec![],
            },
        ];

        let scan_result = scan_folders_with_logging(app.handle(), folders)
            .await
            .unwrap();

        let scanning_logs = logs.with_key("log_scanning_folder");
        assert_eq!(scanning_logs.len(), 2);
        assert_eq!(scanning_logs[0]["level"], "INFO");
        assert_eq!(
            scanning_logs[0]["message"],
            format!("Scanning folder: {}", first.to_string_lossy())
        );
        assert_eq!(
            scanning_logs[0]["params"]["folder"],
            first.to_string_lossy().as_ref()
        );
        assert_eq!(
            scanning_logs[1]["params"]["folder"],
            second.to_string_lossy().as_ref()
        );

        let discovered_paths = scan_result
            .files
            .iter()
            .map(|asset| std::path::PathBuf::from(&asset.path))
            .collect::<Vec<_>>();
        assert_eq!(discovered_paths.len(), 2);
        assert!(discovered_paths.contains(&first.join("keep.jpg")));
        assert!(discovered_paths.contains(&second.join("second.jpg")));
        assert!(!discovered_paths.contains(&first.join("skip.jpg")));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn scan_folders_with_logging_emits_failure_log_key_on_scan_error() {
        let app = tauri::test::mock_app();
        let logs = crate::app::events::CapturedUiLogs::listen(app.handle());
        let folder = db::WatchedFolder {
            id: 1,
            path: "C:/photos".to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec![],
        };

        let err = scan_folders_with_logging_using(app.handle(), vec![folder], |_| async {
            Err("scan exploded".to_string())
        })
        .await
        .unwrap_err();

        assert_eq!(err, "scan exploded");
        assert_eq!(logs.with_key("log_scanning_folder").len(), 1);
        let failure_logs = logs.with_key("log_scan_folders_failed");
        assert_eq!(failure_logs.len(), 1);
        assert_eq!(failure_logs[0]["level"], "ERROR");
        assert_eq!(
            failure_logs[0]["message"],
            "Folder scanning failed: scan exploded"
        );
        assert_eq!(failure_logs[0]["params"]["error"], "scan exploded");
    }

    #[test]
    fn manual_sync_prelude_checks_folders_before_logging_starting_sync() {
        let app = tauri::test::mock_app();
        let logs = crate::app::events::CapturedUiLogs::listen(app.handle());

        assert_eq!(
            prepare_manual_sync(app.handle(), &[]),
            Err("Add at least one folder before starting a sync.".to_string())
        );
        assert!(logs.with_key("log_starting_sync").is_empty());

        let folders = vec![db::WatchedFolder {
            id: 1,
            path: "C:/photos".to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec![],
        }];
        prepare_manual_sync(app.handle(), &folders).unwrap();
        let started_logs = logs.with_key("log_starting_sync");
        assert_eq!(started_logs.len(), 1);
        assert_eq!(started_logs[0]["level"], "INFO");
        assert_eq!(
            started_logs[0]["message"],
            "Starting manual synchronization..."
        );
    }
}
