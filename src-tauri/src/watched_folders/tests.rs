use super::internal::{
    FolderWatcher, NotifyFolderWatcher, RegistrationEffects, SqliteFolderPersistence,
};
use super::{RegisterFolderParams, RemovableVolumeBinding, WatchedFolderRegistration};
use crate::audit::AuditEvent;
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

#[derive(Default, Clone)]
struct RecordedEffects {
    sync_enabled: Arc<Mutex<bool>>,
    triggered_syncs: Arc<Mutex<Vec<String>>>,
    deferred_warnings: Arc<Mutex<Vec<String>>>,
    undo_errors: Arc<Mutex<Vec<(String, String)>>>,
    audit_events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl RegistrationEffects for &RecordedEffects {
    fn enable_sync(&self) {
        *self.sync_enabled.lock().unwrap() = true;
    }

    fn trigger_folder_added_sync(&self, path: String) {
        self.triggered_syncs.lock().unwrap().push(path);
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

    fn record_audit(&self, event: AuditEvent) {
        self.audit_events.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn registers_available_watched_folder_with_sanitized_target_album_and_triggers_sync() {
    let pool = test_pool().await;
    let watcher = test_watcher();
    let effects = RecordedEffects::default();
    let temp = TempDir::new("available");
    let raw_path = format!("{}/", temp.path().to_string_lossy());

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
        &effects,
    );

    let folder_id = registration
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

    assert!(*effects.sync_enabled.lock().unwrap());
    assert_eq!(
        *effects.triggered_syncs.lock().unwrap(),
        vec![expected_path.clone()]
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

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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

    assert!(*effects.sync_enabled.lock().unwrap());
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

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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
    assert!(!*effects.sync_enabled.lock().unwrap());
    assert!(effects.triggered_syncs.lock().unwrap().is_empty());
    assert!(effects.audit_events.lock().unwrap().is_empty());
    assert!(effects.undo_errors.lock().unwrap().is_empty());

    let failing_unwatch_registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        FailingUnwatchWatcher {
            inner: NotifyFolderWatcher { watcher: &watcher },
        },
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

    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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

    assert!(!*effects.sync_enabled.lock().unwrap());
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
    let registration = WatchedFolderRegistration::new(
        SqliteFolderPersistence { pool: &pool },
        NotifyFolderWatcher { watcher: &watcher },
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
