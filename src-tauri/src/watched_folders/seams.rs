use crate::audit::AuditEvent;
use crate::db::{self, RemovableFolderBinding, WatchedFolder};
use crate::immich::AlbumId;
use crate::watcher::{self, WatchRegistration, WatcherState};
use tauri::Manager;

/// Validated, trimmed, and case-insensitively deduplicated exclusion patterns for a Watched Folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExclusionPatterns(Vec<String>);

impl ExclusionPatterns {
    pub(crate) fn parse(patterns: Vec<String>) -> Result<Self, String> {
        crate::media::admission::normalize_patterns(patterns).map(Self)
    }

    pub(crate) fn as_slice(&self) -> &[String] {
        &self.0
    }
}

pub(crate) trait FolderPersistence {
    async fn list_folders(&self) -> Result<Vec<WatchedFolder>, String>;
    async fn list_removable_bindings_by_volume(
        &self,
        volume_id: &str,
    ) -> Result<Vec<RemovableFolderBinding>, String>;
    async fn add_folder(
        &self,
        path: &str,
        target_album_id: Option<&AlbumId>,
    ) -> Result<i64, String>;
    async fn bind_removable_folder(
        &self,
        folder_id: i64,
        volume_id: &str,
        relative_path: &str,
    ) -> Result<(), String>;
    async fn update_exclusion_patterns(
        &self,
        folder_id: i64,
        exclusion_patterns: &ExclusionPatterns,
    ) -> Result<bool, String>;
    async fn remove_folder(&self, folder_id: i64) -> Result<(), String>;
}

pub(crate) trait FolderWatcher: Send + Sync {
    fn watch_available(
        &self,
        path: &str,
    ) -> impl std::future::Future<Output = Result<WatchRegistration, String>> + Send;
    fn unwatch(&self, path: &str) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

pub(crate) trait FolderScanTrigger: Send + Sync {
    fn trigger_folder_scan(&self, folder: WatchedFolder, source: crate::sync::SyncSource);
}

pub(crate) trait FolderSyncWork {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String>;
    fn cancel_work_in_folder(&self, path: &str);
    async fn discard_folder_queue(&self, path: &str) -> Result<(), String>;
}

pub(crate) trait FolderEffects: FolderScanTrigger {
    fn record_audit(&self, event: AuditEvent);
    fn set_sync_enabled(&self, enabled: bool);
    async fn scan_and_admit(
        &self,
        folder: WatchedFolder,
        source: crate::sync::SyncSource,
    ) -> Result<(), String>;
    fn log_watcher_deferred(&self, path: &str);
    fn log_watcher_undo_failed(&self, path: &str, error: &str);
    fn log_unwatch_failed(&self, path: &str, error: &str);
    fn log_watcher_restore_failed(&self, path: &str, error: &str);
    fn log_tray_refresh_after_remove_failed(&self, error: &str);
}

pub(crate) struct SqliteFolderPersistence<'a> {
    pub(crate) pool: &'a sqlx::SqlitePool,
}

impl FolderPersistence for SqliteFolderPersistence<'_> {
    async fn list_folders(&self) -> Result<Vec<WatchedFolder>, String> {
        db::get_folders(self.pool)
            .await
            .map_err(|error| error.to_string())
    }

    async fn list_removable_bindings_by_volume(
        &self,
        volume_id: &str,
    ) -> Result<Vec<RemovableFolderBinding>, String> {
        db::get_removable_folders_by_volume(self.pool, volume_id)
            .await
            .map_err(|error| error.to_string())
    }

    async fn add_folder(
        &self,
        path: &str,
        target_album_id: Option<&AlbumId>,
    ) -> Result<i64, String> {
        db::add_folder(self.pool, path, target_album_id)
            .await
            .map_err(|error| error.to_string())
    }

    async fn bind_removable_folder(
        &self,
        folder_id: i64,
        volume_id: &str,
        relative_path: &str,
    ) -> Result<(), String> {
        db::bind_removable_folder(self.pool, folder_id, volume_id, relative_path)
            .await
            .map_err(|error| error.to_string())
    }

    async fn update_exclusion_patterns(
        &self,
        folder_id: i64,
        exclusion_patterns: &ExclusionPatterns,
    ) -> Result<bool, String> {
        db::update_exclusion_patterns(self.pool, folder_id, exclusion_patterns.as_slice())
            .await
            .map_err(|error| error.to_string())
    }

    async fn remove_folder(&self, folder_id: i64) -> Result<(), String> {
        db::remove_folder(self.pool, folder_id)
            .await
            .map_err(|error| error.to_string())
    }
}

pub(crate) struct NotifyFolderWatcher<'a> {
    pub(crate) watcher: &'a tokio::sync::Mutex<WatcherState>,
}

impl<'a> NotifyFolderWatcher<'a> {
    pub(crate) fn for_tauri(app: &'a tauri::AppHandle) -> Self {
        Self {
            watcher: app.state::<tokio::sync::Mutex<WatcherState>>().inner(),
        }
    }
}

impl FolderWatcher for NotifyFolderWatcher<'_> {
    async fn watch_available(&self, path: &str) -> Result<WatchRegistration, String> {
        let mut watcher_guard = self.watcher.lock().await;
        watcher_guard
            .reconcile_path(path, true)
            .map_err(|error| format!("Could not watch '{path}': {error}"))
    }

    async fn unwatch(&self, path: &str) -> Result<(), String> {
        let mut watcher_guard = self.watcher.lock().await;
        watcher::unwatch_path(&mut watcher_guard, path).map_err(|error| error.to_string())
    }
}

pub(crate) struct CoordinatorSyncWork<'a> {
    pub(crate) pool: &'a sqlx::SqlitePool,
    pub(crate) running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) coordinator: std::sync::Arc<crate::sync::SyncCoordinatorInner>,
}

impl FolderSyncWork for CoordinatorSyncWork<'_> {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String> {
        if self.coordinator.active_work.has_work_in(path) {
            return Ok(true);
        }
        db::has_sync_work_in_folder(self.pool, path)
            .await
            .map_err(|error| error.to_string())
    }

    fn cancel_work_in_folder(&self, path: &str) {
        self.coordinator.active_work.cancel_work_in(path);
    }

    async fn discard_folder_queue(&self, path: &str) -> Result<(), String> {
        crate::sync::lifecycle::SynchronizationLifecycle::new(
            self.pool,
            &self.running,
            self.coordinator.clone(),
        )
        .discard_folder(path)
        .await
        .map_err(|error| error.to_string())
    }
}

async fn scan_single_folder(
    folder: WatchedFolder,
) -> Result<crate::media::model::ScanResult, String> {
    crate::media::scanner::scan_watched_folders_for_media(vec![folder]).await
}

pub(crate) struct TauriFolderScanTrigger<'a> {
    pub(crate) app: &'a tauri::AppHandle,
    pub(crate) pool: &'a sqlx::SqlitePool,
}

impl FolderScanTrigger for TauriFolderScanTrigger<'_> {
    fn trigger_folder_scan(&self, folder: WatchedFolder, source: crate::sync::SyncSource) {
        spawn_folder_scan(self.app, self.pool, folder, source);
    }
}

fn spawn_folder_scan(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    folder: WatchedFolder,
    source: crate::sync::SyncSource,
) {
    let pool_inner = pool.clone();
    let app_for_scan = app.clone();
    tauri::async_runtime::spawn(async move {
        let scan_result = match scan_single_folder(folder.clone()).await {
            Ok(result) => result,
            Err(error) => {
                crate::app::events::log_to_ui_folder_error(
                    &app_for_scan,
                    "ERROR",
                    &format!("Folder scanning failed for '{}': {error}", folder.path),
                    "log_folder_scan_failed",
                    &folder.path,
                    &error,
                );
                return;
            }
        };
        crate::sync::admit_and_run(app_for_scan, pool_inner, scan_result, source).await;
    });
}

pub(crate) struct TauriFolderEffects<'a> {
    pub(crate) app: &'a tauri::AppHandle,
    pub(crate) pool: &'a sqlx::SqlitePool,
}

impl FolderScanTrigger for TauriFolderEffects<'_> {
    fn trigger_folder_scan(&self, folder: WatchedFolder, source: crate::sync::SyncSource) {
        spawn_folder_scan(self.app, self.pool, folder, source);
    }
}

impl FolderEffects for TauriFolderEffects<'_> {
    fn record_audit(&self, event: AuditEvent) {
        crate::audit::audit_event(self.app, event);
    }

    fn set_sync_enabled(&self, enabled: bool) {
        crate::app::tray::set_sync_enabled(self.app, enabled);
    }

    async fn scan_and_admit(
        &self,
        folder: WatchedFolder,
        source: crate::sync::SyncSource,
    ) -> Result<(), String> {
        crate::sync::Synchronization::for_tauri(self.app, self.pool)
            .scan_and_admit(vec![folder], source)
            .await
            .map(|_| ())
    }

    fn log_watcher_deferred(&self, path: &str) {
        crate::app::events::log_to_ui_folder(
            self.app,
            "WARN",
            &format!("Folder '{path}' does not exist; watching deferred until it appears."),
            "log_folder_deferred",
            path,
        );
    }

    fn log_watcher_undo_failed(&self, path: &str, error: &str) {
        crate::app::events::log_to_ui_folder_error(
            self.app,
            "ERROR",
            &format!("Could not undo watcher registration for '{path}': {error}"),
            "log_watcher_undo_failed",
            path,
            error,
        );
    }

    fn log_unwatch_failed(&self, path: &str, error: &str) {
        crate::app::events::log_to_ui_folder_error(
            self.app,
            "WARN",
            &format!("Failed to unwatch '{path}': {error}"),
            "log_unwatch_failed",
            path,
            error,
        );
    }

    fn log_watcher_restore_failed(&self, path: &str, error: &str) {
        crate::app::events::log_to_ui_folder_error(
            self.app,
            "ERROR",
            &format!("Could not restore watcher registration for '{path}': {error}"),
            "log_watcher_restore_failed",
            path,
            error,
        );
    }

    fn log_tray_refresh_after_remove_failed(&self, error: &str) {
        crate::app::events::log_to_ui_error(
            self.app,
            "ERROR",
            &format!("Folder was removed, but could not refresh tray sync availability: {error}"),
            "log_tray_refresh_after_remove_failed",
            error,
        );
    }
}
