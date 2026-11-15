use super::seams::{
    CoordinatorSyncWork, ExclusionPatterns, FolderEffects, FolderPersistence, FolderScanTrigger,
    FolderSyncWork, FolderWatcher, NotifyFolderWatcher, SqliteFolderPersistence,
};
use super::{RegisterFolderParams, RemovableVolumeBinding, RemoveFolderResult, WatchedFolders};
use crate::audit::AuditEvent;
use crate::db::{RemovableFolderBinding, WatchedFolder};
use crate::immich::AlbumId;
use crate::removable::{ActiveRemovableVolumes, VolumeInfo};
use crate::watcher::{create_watcher, WatchRegistration, WatcherState};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};

async fn test_pool() -> SqlitePool {
    let database_name = format!("lymic_watched_folders_test_{}", uuid::Uuid::new_v4());
    let uri = format!("sqlite:file:{database_name}?mode=memory&cache=private");
    let options: SqliteConnectOptions = uri.parse().unwrap();
    let options = options.foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn test_watcher() -> tokio::sync::Mutex<WatcherState> {
    let (tx, _rx) = mpsc::channel(1);
    tokio::sync::Mutex::new(WatcherState::new(
        create_watcher(
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
        )
        .unwrap(),
    ))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lymic-wf-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Clone)]
struct TriggeredSync {
    folder: crate::db::WatchedFolder,
    source: crate::sync::SyncSource,
}

#[derive(Default, Clone)]
struct RecordedEffects {
    tray_sync_enabled: Arc<Mutex<Vec<bool>>>,
    triggered_syncs: Arc<Mutex<Vec<TriggeredSync>>>,
    admitted_scans: Arc<Mutex<Vec<(crate::sync::ScanResult, crate::sync::SyncSource)>>>,
    deferred_warnings: Arc<Mutex<Vec<String>>>,
    undo_errors: Arc<Mutex<Vec<(String, String)>>>,
    unwatch_errors: Arc<Mutex<Vec<(String, String)>>>,
    restore_errors: Arc<Mutex<Vec<(String, String)>>>,
    tray_refresh_errors: Arc<Mutex<Vec<String>>>,
    audit_events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl RecordedEffects {
    fn last_sync_enabled(&self) -> bool {
        self.tray_sync_enabled
            .lock()
            .unwrap()
            .last()
            .copied()
            .unwrap_or(false)
    }
}

#[derive(Clone, Default)]
struct ConfiguredFolderPersistence {
    folder: Arc<Mutex<Option<WatchedFolder>>>,
}

impl FolderPersistence for ConfiguredFolderPersistence {
    async fn list_folders(&self) -> Result<Vec<WatchedFolder>, String> {
        Ok(self.folder.lock().unwrap().iter().cloned().collect())
    }

    async fn list_removable_bindings_by_volume(
        &self,
        _volume_id: &str,
    ) -> Result<Vec<RemovableFolderBinding>, String> {
        Ok(Vec::new())
    }

    async fn add_folder(
        &self,
        path: &str,
        target_album_id: Option<&AlbumId>,
    ) -> Result<i64, String> {
        *self.folder.lock().unwrap() = Some(WatchedFolder {
            id: 1,
            path: path.to_string(),
            recursive: true,
            target_album_id: target_album_id.cloned(),
            exclusion_patterns: vec!["private.jpg".to_string()],
        });
        Ok(1)
    }

    async fn bind_removable_folder(
        &self,
        _folder_id: i64,
        _volume_id: &str,
        _relative_path: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn update_exclusion_patterns(
        &self,
        folder_id: i64,
        exclusion_patterns: &ExclusionPatterns,
    ) -> Result<bool, String> {
        let mut guard = self.folder.lock().unwrap();
        if let Some(folder) = guard.as_mut().filter(|folder| folder.id == folder_id) {
            folder.exclusion_patterns = exclusion_patterns.as_slice().to_vec();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn remove_folder(&self, _folder_id: i64) -> Result<(), String> {
        *self.folder.lock().unwrap() = None;
        Ok(())
    }
}

impl FolderScanTrigger for &RecordedEffects {
    fn trigger_folder_scan(
        &self,
        folder: crate::db::WatchedFolder,
        source: crate::sync::SyncSource,
    ) {
        self.triggered_syncs
            .lock()
            .unwrap()
            .push(TriggeredSync { folder, source });
    }
}

impl FolderEffects for &RecordedEffects {
    fn record_audit(&self, event: AuditEvent) {
        self.audit_events.lock().unwrap().push(event);
    }

    fn set_sync_enabled(&self, enabled: bool) {
        self.tray_sync_enabled.lock().unwrap().push(enabled);
    }

    async fn admit_rescan(
        &self,
        scan_result: crate::sync::ScanResult,
        source: crate::sync::SyncSource,
    ) {
        self.admitted_scans
            .lock()
            .unwrap()
            .push((scan_result, source));
    }

    fn log_watcher_deferred(&self, path: &str) {
        self.deferred_warnings
            .lock()
            .unwrap()
            .push(path.to_string());
    }

    fn log_watcher_undo_failed(&self, path: &str, error: &str) {
        self.undo_errors
            .lock()
            .unwrap()
            .push((path.to_string(), error.to_string()));
    }

    fn log_unwatch_failed(&self, path: &str, error: &str) {
        self.unwatch_errors
            .lock()
            .unwrap()
            .push((path.to_string(), error.to_string()));
    }

    fn log_watcher_restore_failed(&self, path: &str, error: &str) {
        self.restore_errors
            .lock()
            .unwrap()
            .push((path.to_string(), error.to_string()));
    }

    fn log_tray_refresh_after_remove_failed(&self, error: &str) {
        self.tray_refresh_errors
            .lock()
            .unwrap()
            .push(error.to_string());
    }
}

fn test_sync_work(pool: &SqlitePool) -> CoordinatorSyncWork<'_> {
    CoordinatorSyncWork {
        pool,
        running: Arc::new(AtomicBool::new(false)),
        coordinator: Arc::new(crate::sync::SyncCoordinatorInner::new(false)),
    }
}

#[tokio::test]
async fn registers_available_watched_folder_with_sanitized_target_album_and_triggers_sync() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("available");
    let raw_path = format!("{}/", temp.path().to_string_lossy());

    let folders = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    let folder_id = folders
        .register(RegisterFolderParams::new(
            &raw_path,
            Some("  album-uuid-1  ".to_string()),
        ))
        .await
        .unwrap();

    let expected_path = crate::media::paths::canonicalize_folder_path(temp.path())
        .to_string_lossy()
        .to_string();

    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder_id);
    assert_eq!(folders[0].path, expected_path);
    assert_eq!(
        folders[0].target_album_id.as_ref().map(|id| id.as_str()),
        Some("album-uuid-1")
    );

    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );

    assert!(effects.last_sync_enabled());
    let triggered_syncs = effects.triggered_syncs.lock().unwrap();
    assert_eq!(triggered_syncs.len(), 1);
    assert_eq!(triggered_syncs[0].folder.path, expected_path);
    assert_eq!(
        triggered_syncs[0]
            .folder
            .target_album_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("album-uuid-1")
    );
    assert!(triggered_syncs[0].folder.exclusion_patterns.is_empty());
    assert_eq!(
        triggered_syncs[0].source,
        crate::sync::SyncSource::FolderAdded
    );
    assert!(effects.deferred_warnings.lock().unwrap().is_empty());

    let audits = effects.audit_events.lock().unwrap();
    assert_eq!(audits.len(), 1);
    let audit_json = serde_json::to_value(&audits[0]).unwrap();
    assert_eq!(audit_json["event_type"], "folder.added");
    assert_eq!(audit_json["details"]["folder_id"], folder_id);
    assert_eq!(audit_json["details"]["local_path"], expected_path);
    assert_eq!(audit_json["details"]["target_album_id"], "album-uuid-1");
}

#[tokio::test]
async fn initial_folder_sync_receives_persisted_exclusion_patterns() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("persisted-exclusions");
    let folders = WatchedFolders::new(
        ConfiguredFolderPersistence::default(),
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    folders
        .register(RegisterFolderParams::new(
            temp.path().to_string_lossy().to_string(),
            None,
        ))
        .await
        .unwrap();

    let triggered_syncs = effects.triggered_syncs.lock().unwrap();
    assert_eq!(triggered_syncs.len(), 1);
    assert_eq!(
        triggered_syncs[0].folder.exclusion_patterns,
        ["private.jpg"]
    );
}

#[tokio::test]
async fn rejects_duplicate_overlapping_and_non_directory_paths_while_admitting_siblings() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let root = TempDir::new("conflicts");
    let photos = root.path().join("Photos");
    let child = photos.join("2026");
    let sibling = root.path().join("Photos-Vacation");
    let regular_file = root.path().join("not-a-folder.jpg");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(&regular_file, b"image").unwrap();

    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    registration
        .register(RegisterFolderParams::new(
            photos.to_string_lossy(),
            Some("   ".to_string()),
        ))
        .await
        .unwrap();

    // Exact duplicate
    let duplicate_err = registration
        .register(RegisterFolderParams::new(photos.to_string_lossy(), None))
        .await
        .unwrap_err();
    assert!(duplicate_err.contains("already being watched"));

    // Child subfolder
    let child_err = registration
        .register(RegisterFolderParams::new(child.to_string_lossy(), None))
        .await
        .unwrap_err();
    assert!(child_err.contains("overlaps with already watched folder"));

    // Parent folder
    let parent_err = registration
        .register(RegisterFolderParams::new(
            root.path().to_string_lossy(),
            None,
        ))
        .await
        .unwrap_err();
    assert!(parent_err.contains("overlaps with already watched folder"));

    // Regular file
    let file_err = registration
        .register(RegisterFolderParams::new(
            regular_file.to_string_lossy(),
            None,
        ))
        .await
        .unwrap_err();
    assert!(file_err.contains("is not a directory"));

    // Sibling with common prefix succeeds
    registration
        .register(RegisterFolderParams::new(sibling.to_string_lossy(), None))
        .await
        .unwrap();

    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 2);
    assert!(folders[0].target_album_id.is_none());
}

#[tokio::test]
async fn unavailable_watched_folder_remains_configured_defers_watching_and_skips_initial_scan() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let missing_path = std::env::temp_dir().join(format!(
        "lymic-wf-missing-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));

    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    let folder_id = registration
        .register(RegisterFolderParams::new(
            missing_path.to_string_lossy(),
            Some("album-offline".to_string()),
        ))
        .await
        .unwrap();

    let expected_path = crate::media::paths::canonicalize_folder_path(&missing_path)
        .to_string_lossy()
        .to_string();

    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder_id);
    assert_eq!(folders[0].path, expected_path);
    assert_eq!(
        folders[0].target_album_id.as_ref().map(|id| id.as_str()),
        Some("album-offline")
    );

    assert!(effects.last_sync_enabled());
    assert_eq!(
        *effects.deferred_warnings.lock().unwrap(),
        vec![expected_path]
    );
    assert!(effects.triggered_syncs.lock().unwrap().is_empty());
    assert_eq!(effects.audit_events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn registers_removable_watched_folder_and_persists_volume_binding() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("removable");
    let dcim = temp.path().join("DCIM").join("100EOS");
    std::fs::create_dir_all(&dcim).unwrap();

    let active_volumes = ActiveRemovableVolumes::new();
    active_volumes.insert(VolumeInfo {
        volume_id: "vol-sd-999".to_string(),
        mount_path: temp.path().to_string_lossy().to_string(),
        display_name: "EOS_DIGITAL".to_string(),
    });

    let input = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-sd-999".to_string(),
        dcim.to_string_lossy().to_string(),
        Some("album-sd".to_string()),
    )
    .unwrap();

    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );
    let folder_id = registration.register(input).await.unwrap();

    let binding = crate::db::get_removable_folder_binding(&pool, folder_id)
        .await
        .unwrap();
    assert_eq!(
        binding,
        Some(crate::db::RemovableFolderBinding {
            watched_folder_id: folder_id,
            volume_id: "vol-sd-999".to_string(),
            relative_path: "DCIM/100EOS".to_string(),
        })
    );
}

struct FailingUnwatchWatcher<'a> {
    inner: NotifyFolderWatcher<'a>,
}

impl FolderWatcher for FailingUnwatchWatcher<'_> {
    async fn watch_available(&self, path: &str) -> Result<WatchRegistration, String> {
        self.inner.watch_available(path).await
    }

    async fn unwatch(&self, _path: &str) -> Result<(), String> {
        Err("simulated unwatch failure".to_string())
    }
}

#[tokio::test]
async fn persistence_failure_undoes_watcher_registration_and_logs_if_undo_fails() {
    let pool = test_pool().await;
    sqlx::query(
        "CREATE TRIGGER fail_watched_folder_insert BEFORE INSERT ON watched_folders \
         BEGIN SELECT RAISE(FAIL, 'simulated persistence failure'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("persist-fail");
    let expected_path = crate::media::paths::canonicalize_folder_path(temp.path())
        .to_string_lossy()
        .to_string();

    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    let err = registration
        .register(RegisterFolderParams::new(
            temp.path().to_string_lossy(),
            None,
        ))
        .await
        .unwrap_err();
    assert!(err.contains("simulated persistence failure"));

    // Watcher registration was undone
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::Registered
    );
    watcher.lock().await.unwatch_path(&expected_path).unwrap();

    assert!(crate::db::get_folders(&pool).await.unwrap().is_empty());
    assert!(!effects.last_sync_enabled());
    assert!(effects.triggered_syncs.lock().unwrap().is_empty());
    assert!(effects.audit_events.lock().unwrap().is_empty());
    assert!(effects.undo_errors.lock().unwrap().is_empty());

    let failing_unwatch_registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        FailingUnwatchWatcher {
            inner: NotifyFolderWatcher { watcher: &watcher },
        },
        test_sync_work(&pool),
        &effects,
    );
    let _ = failing_unwatch_registration
        .register(RegisterFolderParams::new(
            temp.path().to_string_lossy(),
            None,
        ))
        .await
        .unwrap_err();
    assert_eq!(
        *effects.undo_errors.lock().unwrap(),
        vec![(expected_path, "simulated unwatch failure".to_string())]
    );
}

#[tokio::test]
async fn removable_binding_failure_undoes_persisted_folder_and_watcher_registration() {
    let pool = test_pool().await;
    sqlx::query(
        "CREATE TRIGGER fail_removable_binding_insert BEFORE INSERT ON removable_watched_folders \
         BEGIN SELECT RAISE(FAIL, 'simulated binding failure'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("bind-fail");
    let expected_path = crate::media::paths::canonicalize_folder_path(temp.path())
        .to_string_lossy()
        .to_string();

    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );

    let err = registration
        .register(
            RegisterFolderParams::new(temp.path().to_string_lossy(), None)
                .with_removable_binding(RemovableVolumeBinding::new("vol-fail-1", "DCIM")),
        )
        .await
        .unwrap_err();
    assert!(err.contains("simulated binding failure"));

    assert!(!effects.last_sync_enabled());
    assert!(effects.triggered_syncs.lock().unwrap().is_empty());
    assert!(effects.audit_events.lock().unwrap().is_empty());

    // After removing the failure trigger, registering the same folder succeeds cleanly,
    // proving neither a persisted folder row nor a stale watcher registration was left behind.
    sqlx::query("DROP TRIGGER fail_removable_binding_insert;")
        .execute(&pool)
        .await
        .unwrap();

    let folder_id = registration
        .register(
            RegisterFolderParams::new(temp.path().to_string_lossy(), None)
                .with_removable_binding(RemovableVolumeBinding::new("vol-fail-1", "DCIM")),
        )
        .await
        .unwrap();
    assert_eq!(crate::db::get_folders(&pool).await.unwrap().len(), 1);
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    assert!(crate::db::get_removable_folder_binding(&pool, folder_id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn removable_acceptance_validates_presence_bounds_and_containment() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let active_volumes = ActiveRemovableVolumes::new();
    let mount_dir = TempDir::new("sdcard");
    let mount_path = mount_dir.path().to_string_lossy().to_string();
    let dcim_dir = mount_dir.path().join("DCIM").join("100EOS");
    std::fs::create_dir_all(&dcim_dir).unwrap();

    active_volumes.insert(VolumeInfo {
        volume_id: "vol-sd-123".to_string(),
        mount_path: mount_path.clone(),
        display_name: "SD_CARD".to_string(),
    });

    // 1. Disconnected volume
    let err = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-missing".to_string(),
        dcim_dir.to_string_lossy().to_string(),
        None,
    )
    .unwrap_err();
    assert!(err.contains("not connected") || err.contains("not found"));

    // 2. Path outside volume mount
    let outside_dir = TempDir::new("outside");
    let err = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-sd-123".to_string(),
        outside_dir.path().to_string_lossy().to_string(),
        None,
    )
    .unwrap_err();
    assert!(err.contains("does not reside within") || err.contains("outside"));

    // 3. Valid path inside volume
    let params = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-sd-123".to_string(),
        dcim_dir.to_string_lossy().to_string(),
        None,
    )
    .unwrap();
    assert_eq!(
        params
            .removable_binding()
            .unwrap()
            .relative_path()
            .replace('\\', "/"),
        "DCIM/100EOS"
    );

    // 4. Valid path at mount root
    let root_params = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-sd-123".to_string(),
        mount_path.clone(),
        None,
    )
    .unwrap();
    assert_eq!(root_params.removable_binding().unwrap().relative_path(), "");

    // 5. Already bound volume is rejected during registration
    let registration = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        test_sync_work(&pool),
        &effects,
    );
    registration.register(root_params).await.unwrap();

    let second_attempt = RegisterFolderParams::from_removable_acceptance(
        &active_volumes,
        "vol-sd-123".to_string(),
        dcim_dir.to_string_lossy().to_string(),
        None,
    )
    .unwrap();
    let err = registration.register(second_attempt).await.unwrap_err();
    assert!(err.contains("already bound"));
}

#[test]
fn detect_removable_binding_for_path_resolves_mounted_volumes() {
    let active_volumes = ActiveRemovableVolumes::new();
    let volume = VolumeInfo {
        volume_id: "vol-card-456".to_string(),
        mount_path: if cfg!(windows) {
            "E:\\".to_string()
        } else {
            "/media/sdcard".to_string()
        },
        display_name: "SD_CARD".to_string(),
    };
    active_volumes.insert(volume);

    // Path on removable card via RegisterFolderParams::from_local_path
    let card_path = if cfg!(windows) {
        "E:\\DCIM\\100CANON"
    } else {
        "/media/sdcard/DCIM/100CANON"
    };
    let params = RegisterFolderParams::from_local_path(card_path, None, Some(&active_volumes));
    let binding = params
        .removable_binding()
        .expect("removable binding detected");
    assert_eq!(binding.volume_id(), "vol-card-456");
    assert_eq!(binding.relative_path().replace('\\', "/"), "DCIM/100CANON");

    // Path on fixed local drive
    let fixed_path = if cfg!(windows) {
        "C:\\Users\\Photos"
    } else {
        "/home/user/Photos"
    };
    let fixed_params =
        RegisterFolderParams::from_local_path(fixed_path, None, Some(&active_volumes));
    assert!(fixed_params.removable_binding().is_none());
}

async fn register_test_folder(
    pool: &SqlitePool,
    watcher: &tokio::sync::Mutex<WatcherState>,
    path: &Path,
) -> (i64, String) {
    let reg_effects = RecordedEffects::default();
    let folders = WatchedFolders::new(
        SqliteFolderPersistence { pool },
        NotifyFolderWatcher { watcher },
        test_sync_work(pool),
        &reg_effects,
    );
    let folder_id = folders
        .register(RegisterFolderParams::new(path.to_string_lossy(), None))
        .await
        .unwrap();
    let expected_path = crate::media::paths::canonicalize_folder_path(path)
        .to_string_lossy()
        .to_string();
    (folder_id, expected_path)
}

fn test_upload_unit(folder: &str, file: &str) -> crate::media::model::UploadUnit {
    crate::media::model::UploadUnit {
        image: crate::media::model::Asset {
            path: PathBuf::from(folder)
                .join(file)
                .to_string_lossy()
                .to_string(),
            size: 100,
            mtime: 1,
            target_album_id: None,
            remote_id: None,
        },
        live_photo_video: None,
    }
}

async fn enqueue_test_files(pool: &SqlitePool, paths: &[PathBuf]) {
    let units: Vec<_> = paths
        .iter()
        .map(|path| crate::db::QueueUploadUnit {
            image: crate::db::QueueUploadAsset {
                path: path.to_string_lossy().to_string(),
                size: 200,
                mtime: 2,
                target_album_id: None,
            },
            live_photo_video: None,
        })
        .collect();
    crate::db::enqueue_upload_units(pool, &units, &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn unconfirmed_removal_changes_nothing_and_reports_whether_sync_work_would_be_cancelled() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("remove-unconfirmed");
    let (folder_id, expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator: coordinator.clone(),
        },
        &removal_effects,
    );

    // 1. No sync work: None and Some(false) report false and change nothing.
    assert_eq!(
        removal.remove(folder_id, None).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: false
        }
    );
    assert_eq!(
        removal.remove(folder_id, Some(false)).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: false
        }
    );
    assert_eq!(crate::db::get_folders(&pool).await.unwrap().len(), 1);
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());

    // 2. Active in-flight work: reports true and does not cancel active work or remove folder.
    let active_unit = test_upload_unit(&expected_path, "active.jpg");
    let lease = coordinator
        .active_work
        .acquire(std::slice::from_ref(&active_unit));
    assert_eq!(
        removal.remove(folder_id, None).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: true
        }
    );
    assert!(!lease.context().for_upload_unit(&active_unit).is_cancelled());
    assert_eq!(crate::db::get_folders(&pool).await.unwrap().len(), 1);
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());
    drop(lease);

    // 3. Queued work in DB: reports true and leaves queue and folder untouched.
    enqueue_test_files(&pool, &[PathBuf::from(&expected_path).join("queued.jpg")]).await;
    assert_eq!(
        removal.remove(folder_id, Some(false)).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: true
        }
    );
    assert!(crate::db::has_sync_work_in_folder(&pool, &expected_path)
        .await
        .unwrap());
    assert_eq!(crate::db::get_folders(&pool).await.unwrap().len(), 1);
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());
}

struct OrderedWatcher<'a> {
    inner: NotifyFolderWatcher<'a>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FolderWatcher for OrderedWatcher<'_> {
    async fn watch_available(&self, path: &str) -> Result<WatchRegistration, String> {
        self.inner.watch_available(path).await
    }

    async fn unwatch(&self, path: &str) -> Result<(), String> {
        self.events.lock().unwrap().push("unwatch");
        self.inner.unwatch(path).await
    }
}

struct OrderedSync<'a> {
    inner: CoordinatorSyncWork<'a>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FolderSyncWork for OrderedSync<'_> {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String> {
        self.inner.has_work_in_folder(path).await
    }

    fn cancel_work_in_folder(&self, path: &str) {
        self.events.lock().unwrap().push("cancel");
        self.inner.cancel_work_in_folder(path);
    }

    async fn discard_folder_queue(&self, path: &str) -> Result<(), String> {
        self.events.lock().unwrap().push("discard");
        self.inner.discard_folder_queue(path).await
    }

    async fn scan_folder(&self, folder: WatchedFolder) -> Result<crate::sync::ScanResult, String> {
        self.inner.scan_folder(folder).await
    }
}

#[tokio::test]
async fn confirmed_removal_stops_watcher_before_cancelling_and_discarding_then_deletes_and_audits()
{
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("remove-confirmed");
    let (folder_id, expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    let active_unit = test_upload_unit(&expected_path, "active.jpg");
    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let lease = coordinator
        .active_work
        .acquire(std::slice::from_ref(&active_unit));

    enqueue_test_files(&pool, &[PathBuf::from(&expected_path).join("queued.jpg")]).await;

    let events = Arc::new(Mutex::new(Vec::new()));
    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        OrderedWatcher {
            inner: NotifyFolderWatcher { watcher: &watcher },
            events: events.clone(),
        },
        OrderedSync {
            inner: CoordinatorSyncWork {
                pool: &pool,
                running: running.clone(),
                coordinator: coordinator.clone(),
            },
            events: events.clone(),
        },
        &removal_effects,
    );

    let result = removal.remove(folder_id, Some(true)).await.unwrap();
    assert_eq!(
        result,
        RemoveFolderResult {
            requires_sync_cancellation: false
        }
    );

    // 1. Order: watcher stopped before active work cancelled and queue discarded
    assert_eq!(
        *events.lock().unwrap(),
        vec!["unwatch", "cancel", "discard"]
    );

    // 2. Watcher was actually stopped
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::Registered
    );
    watcher.lock().await.unwatch_path(&expected_path).unwrap();

    // 3. Active work cancelled, queued work discarded, folder deleted
    assert!(lease.context().for_upload_unit(&active_unit).is_cancelled());
    drop(lease);
    assert!(!crate::db::has_sync_work_in_folder(&pool, &expected_path)
        .await
        .unwrap());
    assert!(crate::db::get_folders(&pool).await.unwrap().is_empty());

    // 4. Tray sync availability refreshed to false and audit event recorded
    assert_eq!(
        *removal_effects.tray_sync_enabled.lock().unwrap(),
        vec![false]
    );
    let audits = removal_effects.audit_events.lock().unwrap();
    assert_eq!(audits.len(), 1);
    let audit_json = serde_json::to_value(&audits[0]).unwrap();
    assert_eq!(audit_json["event_type"], "folder.removed");
    assert_eq!(audit_json["details"]["folder_id"], folder_id);
}

#[tokio::test]
async fn unwatch_failure_aborts_removal_before_cancelling_or_deleting() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("remove-unwatch-fail");
    let (folder_id, expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let active_unit = test_upload_unit(&expected_path, "active.jpg");
    let lease = coordinator
        .active_work
        .acquire(std::slice::from_ref(&active_unit));

    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        FailingUnwatchWatcher {
            inner: NotifyFolderWatcher { watcher: &watcher },
        },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator: coordinator.clone(),
        },
        &removal_effects,
    );

    let err = removal.remove(folder_id, Some(true)).await.unwrap_err();
    assert!(err.contains("Failed to unwatch"));
    assert!(err.contains("simulated unwatch failure"));
    assert!(!lease.context().for_upload_unit(&active_unit).is_cancelled());
    drop(lease);
    assert_eq!(crate::db::get_folders(&pool).await.unwrap().len(), 1);
    assert_eq!(
        *removal_effects.unwatch_errors.lock().unwrap(),
        vec![(expected_path, "simulated unwatch failure".to_string())]
    );
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());
}

struct FailingDiscardSync<'a> {
    inner: CoordinatorSyncWork<'a>,
}

impl FolderSyncWork for FailingDiscardSync<'_> {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String> {
        self.inner.has_work_in_folder(path).await
    }

    fn cancel_work_in_folder(&self, path: &str) {
        self.inner.cancel_work_in_folder(path);
    }

    async fn discard_folder_queue(&self, _path: &str) -> Result<(), String> {
        Err("simulated queue discard failure".to_string())
    }

    async fn scan_folder(&self, folder: WatchedFolder) -> Result<crate::sync::ScanResult, String> {
        self.inner.scan_folder(folder).await
    }
}

#[tokio::test]
async fn queue_discard_failure_restores_watcher_and_keeps_folder_configured() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("remove-discard-fail");
    let (folder_id, expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        FailingDiscardSync {
            inner: CoordinatorSyncWork {
                pool: &pool,
                running: running.clone(),
                coordinator,
            },
        },
        &removal_effects,
    );

    let err = removal.remove(folder_id, Some(true)).await.unwrap_err();
    assert!(err.contains("simulated queue discard failure"));

    // Watcher registration was restored and folder remains configured
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder_id);
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());
}

struct FailingRewatchWatcher<'a> {
    inner: NotifyFolderWatcher<'a>,
}

impl FolderWatcher for FailingRewatchWatcher<'_> {
    async fn watch_available(&self, _path: &str) -> Result<WatchRegistration, String> {
        Err("simulated rewatch failure".to_string())
    }

    async fn unwatch(&self, path: &str) -> Result<(), String> {
        self.inner.unwatch(path).await
    }
}

#[tokio::test]
async fn delete_failure_restores_watcher_keeps_folder_and_logs_if_restore_fails() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("remove-delete-fail");
    let (folder_id, expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    sqlx::query(
        "CREATE TRIGGER fail_watched_folder_delete BEFORE DELETE ON watched_folders \
         BEGIN SELECT RAISE(FAIL, 'simulated delete failure'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator: coordinator.clone(),
        },
        &removal_effects,
    );

    let err = removal.remove(folder_id, Some(true)).await.unwrap_err();
    assert!(err.contains("simulated delete failure"));

    // Watcher registration was restored and folder remains configured
    assert_eq!(
        watcher.lock().await.watch_path(&expected_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder_id);
    assert!(removal_effects.restore_errors.lock().unwrap().is_empty());
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());

    // If restoring the watcher also fails, the failure is logged without masking the delete error
    let failing_restore_removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        FailingRewatchWatcher {
            inner: NotifyFolderWatcher { watcher: &watcher },
        },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator,
        },
        &removal_effects,
    );
    let err = failing_restore_removal
        .remove(folder_id, Some(true))
        .await
        .unwrap_err();
    assert!(err.contains("simulated delete failure"));
    assert_eq!(
        *removal_effects.restore_errors.lock().unwrap(),
        vec![(expected_path, "simulated rewatch failure".to_string())]
    );
}

#[tokio::test]
async fn removing_one_folder_leaves_sibling_folder_queue_active_work_and_watcher_untouched() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let root = TempDir::new("remove-sibling");
    let photos = root.path().join("Photos");
    let sibling = root.path().join("Photos-Vacation");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    let (photos_id, photos_path) = register_test_folder(&pool, &watcher, &photos).await;
    let (sibling_id, sibling_path) = register_test_folder(&pool, &watcher, &sibling).await;

    let photos_active = test_upload_unit(&photos_path, "a.jpg");
    let sibling_active = test_upload_unit(&sibling_path, "b.jpg");

    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let lease = coordinator
        .active_work
        .acquire(&[photos_active.clone(), sibling_active.clone()]);

    enqueue_test_files(
        &pool,
        &[
            PathBuf::from(&photos_path).join("queued_a.jpg"),
            PathBuf::from(&sibling_path).join("queued_b.jpg"),
        ],
    )
    .await;

    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator: coordinator.clone(),
        },
        &removal_effects,
    );

    removal.remove(photos_id, Some(true)).await.unwrap();

    // Target folder work is cancelled & discarded; sibling folder work is untouched
    assert!(lease
        .context()
        .for_upload_unit(&photos_active)
        .is_cancelled());
    assert!(!lease
        .context()
        .for_upload_unit(&sibling_active)
        .is_cancelled());
    drop(lease);

    assert!(!crate::db::has_sync_work_in_folder(&pool, &photos_path)
        .await
        .unwrap());
    assert!(crate::db::has_sync_work_in_folder(&pool, &sibling_path)
        .await
        .unwrap());

    // Sibling folder remains configured and watched, and tray sync stays enabled
    let remaining = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, sibling_id);
    assert_eq!(
        watcher.lock().await.watch_path(&sibling_path).unwrap(),
        WatchRegistration::AlreadyWatched
    );
    assert_eq!(
        *removal_effects.tray_sync_enabled.lock().unwrap(),
        vec![true]
    );
}

#[tokio::test]
async fn removing_missing_folder_requires_confirmation_before_auditing_or_refreshing_tray() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let running = Arc::new(AtomicBool::new(false));
    let coordinator = Arc::new(crate::sync::SyncCoordinatorInner::new(false));
    let removal_effects = RecordedEffects::default();
    let removal = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CoordinatorSyncWork {
            pool: &pool,
            running: running.clone(),
            coordinator,
        },
        &removal_effects,
    );

    assert_eq!(
        removal.remove(999, None).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: false
        }
    );
    assert!(removal_effects.tray_sync_enabled.lock().unwrap().is_empty());
    assert!(removal_effects.audit_events.lock().unwrap().is_empty());

    assert_eq!(
        removal.remove(999, Some(true)).await.unwrap(),
        RemoveFolderResult {
            requires_sync_cancellation: false
        }
    );
    assert_eq!(
        *removal_effects.tray_sync_enabled.lock().unwrap(),
        vec![false]
    );
    assert_eq!(removal_effects.audit_events.lock().unwrap().len(), 1);
}

struct CountingScanSync<'a> {
    inner: CoordinatorSyncWork<'a>,
    scanned_folders: Arc<Mutex<Vec<i64>>>,
}

impl FolderSyncWork for CountingScanSync<'_> {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String> {
        self.inner.has_work_in_folder(path).await
    }

    fn cancel_work_in_folder(&self, path: &str) {
        self.inner.cancel_work_in_folder(path);
    }

    async fn discard_folder_queue(&self, path: &str) -> Result<(), String> {
        self.inner.discard_folder_queue(path).await
    }

    async fn scan_folder(&self, folder: WatchedFolder) -> Result<crate::sync::ScanResult, String> {
        self.scanned_folders.lock().unwrap().push(folder.id);
        self.inner.scan_folder(folder).await
    }
}

#[tokio::test]
async fn invalid_exclusion_patterns_are_rejected_before_anything_is_persisted_or_scanned() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("exclusion-invalid");
    let (folder_id, _expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;
    let initial_patterns = vec!["old/**".to_string()];
    crate::db::update_exclusion_patterns(&pool, folder_id, &initial_patterns)
        .await
        .unwrap();

    let effects = RecordedEffects::default();
    let scanned_folders = Arc::new(Mutex::new(Vec::new()));
    let folders = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CountingScanSync {
            inner: test_sync_work(&pool),
            scanned_folders: scanned_folders.clone(),
        },
        &effects,
    );

    for invalid in [
        "../private/**",
        "Screenshots\\**",
        "/private/**",
        "!keep.jpg",
        "*.{jpg,png}",
    ] {
        let err = folders
            .update_exclusion_patterns(folder_id, vec![invalid.to_string()])
            .await
            .unwrap_err();
        assert!(
            err.contains("Invalid exclusion pattern"),
            "unexpected error: {err}"
        );
    }

    let persisted = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(persisted[0].exclusion_patterns, initial_patterns);
    assert!(scanned_folders.lock().unwrap().is_empty());
    assert!(effects.admitted_scans.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_folder_reports_not_found_and_triggers_no_scan() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let temp = TempDir::new("exclusion-unknown");
    std::fs::write(temp.path().join("photo.jpg"), b"img").unwrap();
    let (_folder_id, _expected_path) = register_test_folder(&pool, &watcher, temp.path()).await;

    let effects = RecordedEffects::default();
    let scanned_folders = Arc::new(Mutex::new(Vec::new()));
    let folders = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CountingScanSync {
            inner: test_sync_work(&pool),
            scanned_folders: scanned_folders.clone(),
        },
        &effects,
    );

    let err = folders
        .update_exclusion_patterns(999, vec!["*.mov".to_string()])
        .await
        .unwrap_err();
    assert!(
        err.to_ascii_lowercase().contains("not found"),
        "expected 'not found' error, got: {err}"
    );
    assert!(scanned_folders.lock().unwrap().is_empty());
    assert!(effects.admitted_scans.lock().unwrap().is_empty());
}

#[tokio::test]
async fn successful_exclusion_update_normalizes_persists_and_rescans_only_target_folder() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let root = TempDir::new("exclusion-rescan");
    let target = root.path().join("Target");
    let sibling = root.path().join("Sibling");
    std::fs::create_dir_all(target.join("Screenshots")).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(target.join("keep.jpg"), b"keep").unwrap();
    std::fs::write(target.join("Screenshots").join("excluded.jpg"), b"excluded").unwrap();
    std::fs::write(sibling.join("sibling.jpg"), b"sibling").unwrap();

    let (target_id, target_path) = register_test_folder(&pool, &watcher, &target).await;
    let (sibling_id, _) = register_test_folder(&pool, &watcher, &sibling).await;

    let effects = RecordedEffects::default();
    let scanned_folders = Arc::new(Mutex::new(Vec::new()));
    let folders = WatchedFolders::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        CountingScanSync {
            inner: test_sync_work(&pool),
            scanned_folders: scanned_folders.clone(),
        },
        &effects,
    );

    folders
        .update_exclusion_patterns(
            target_id,
            vec![
                " Screenshots/** ".to_string(),
                "screenshots/**".to_string(),
                "".to_string(),
            ],
        )
        .await
        .unwrap();

    let persisted = crate::db::get_folders(&pool).await.unwrap();
    let updated_target = persisted.iter().find(|f| f.id == target_id).unwrap();
    let untouched_sibling = persisted.iter().find(|f| f.id == sibling_id).unwrap();
    assert_eq!(updated_target.exclusion_patterns, vec!["Screenshots/**"]);
    assert!(untouched_sibling.exclusion_patterns.is_empty());

    assert_eq!(*scanned_folders.lock().unwrap(), vec![target_id]);

    let admitted = effects.admitted_scans.lock().unwrap();
    assert_eq!(admitted.len(), 1);
    assert_eq!(
        admitted[0].1,
        crate::sync::SyncSource::ExclusionPatternsChanged
    );

    let expected_keep =
        crate::media::paths::normalize_path_lexical(&PathBuf::from(&target_path).join("keep.jpg"));
    let scanned_paths: Vec<PathBuf> = admitted[0]
        .0
        .files
        .iter()
        .map(|asset| crate::media::paths::normalize_path_lexical(Path::new(&asset.path)))
        .collect();
    assert_eq!(scanned_paths, vec![expected_keep]);
}

struct FailingScanSync<'a>(CoordinatorSyncWork<'a>);

impl FolderSyncWork for FailingScanSync<'_> {
    async fn has_work_in_folder(&self, path: &str) -> Result<bool, String> {
        self.0.has_work_in_folder(path).await
    }

    fn cancel_work_in_folder(&self, path: &str) {
        self.0.cancel_work_in_folder(path);
    }

    async fn discard_folder_queue(&self, path: &str) -> Result<(), String> {
        self.0.discard_folder_queue(path).await
    }

    async fn scan_folder(&self, _folder: WatchedFolder) -> Result<crate::sync::ScanResult, String> {
        Err("scan exploded".to_string())
    }
}

#[tokio::test]
async fn scan_failure_after_update_keeps_new_patterns_and_admits_nothing() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let persistence = ConfiguredFolderPersistence::default();
    let folder_id = persistence.add_folder("/watched", None).await.unwrap();
    let effects = RecordedEffects::default();
    let folders = WatchedFolders::new(
        persistence.clone(),
        NotifyFolderWatcher { watcher: &watcher },
        FailingScanSync(test_sync_work(&pool)),
        &effects,
    );

    let err = folders
        .update_exclusion_patterns(folder_id, vec!["*.mov".to_string()])
        .await
        .unwrap_err();

    assert_eq!(err, "scan exploded");
    let stored = persistence.list_folders().await.unwrap();
    assert_eq!(stored[0].exclusion_patterns, vec!["*.mov"]);
    assert!(effects.admitted_scans.lock().unwrap().is_empty());
}

/// Persists the update, then reports the folder as gone when listing.
#[derive(Clone, Default)]
struct VanishingFolderPersistence(ConfiguredFolderPersistence);

impl FolderPersistence for VanishingFolderPersistence {
    async fn list_folders(&self) -> Result<Vec<WatchedFolder>, String> {
        Ok(Vec::new())
    }

    async fn list_removable_bindings_by_volume(
        &self,
        volume_id: &str,
    ) -> Result<Vec<RemovableFolderBinding>, String> {
        self.0.list_removable_bindings_by_volume(volume_id).await
    }

    async fn add_folder(
        &self,
        path: &str,
        target_album_id: Option<&AlbumId>,
    ) -> Result<i64, String> {
        self.0.add_folder(path, target_album_id).await
    }

    async fn bind_removable_folder(
        &self,
        folder_id: i64,
        volume_id: &str,
        relative_path: &str,
    ) -> Result<(), String> {
        self.0
            .bind_removable_folder(folder_id, volume_id, relative_path)
            .await
    }

    async fn update_exclusion_patterns(
        &self,
        folder_id: i64,
        exclusion_patterns: &ExclusionPatterns,
    ) -> Result<bool, String> {
        self.0
            .update_exclusion_patterns(folder_id, exclusion_patterns)
            .await
    }

    async fn remove_folder(&self, folder_id: i64) -> Result<(), String> {
        self.0.remove_folder(folder_id).await
    }
}

#[tokio::test]
async fn folder_vanishing_between_update_and_lookup_reports_not_found_and_triggers_no_scan() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let persistence = VanishingFolderPersistence::default();
    let folder_id = persistence.add_folder("/watched", None).await.unwrap();
    let effects = RecordedEffects::default();
    let scanned_folders = Arc::new(Mutex::new(Vec::new()));
    let folders = WatchedFolders::new(
        persistence,
        NotifyFolderWatcher { watcher: &watcher },
        CountingScanSync {
            inner: test_sync_work(&pool),
            scanned_folders: scanned_folders.clone(),
        },
        &effects,
    );

    let err = folders
        .update_exclusion_patterns(folder_id, vec!["*.mov".to_string()])
        .await
        .unwrap_err();

    assert_eq!(err, "Watched folder was not found.");
    assert!(scanned_folders.lock().unwrap().is_empty());
    assert!(effects.admitted_scans.lock().unwrap().is_empty());
}

#[test]
fn exclusion_patterns_are_trimmed_deduplicated_and_validated() {
    let parsed = ExclusionPatterns::parse(vec![
        " Screenshots/** ".into(),
        "screenshots/**".into(),
        "".into(),
    ])
    .unwrap();
    assert_eq!(parsed.as_slice(), ["Screenshots/**"]);
    assert!(ExclusionPatterns::parse(vec!["../private/**".into()]).is_err());
}
