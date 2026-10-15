use super::*;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::sync::{Arc, Mutex};

async fn test_pool() -> SqlitePool {
    let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
    let uri = format!("sqlite:file:{}?mode=memory&cache=private", database_name);
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

#[derive(Default, Clone)]
struct MockEmitter {
    detected: Arc<Mutex<Vec<VolumeInfo>>>,
    reconciled: Arc<Mutex<Vec<(Option<String>, String)>>>,
    scanned: Arc<Mutex<Vec<String>>>,
}

impl VolumeEventEmitter for MockEmitter {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String> {
        self.detected.lock().unwrap().push(volume.clone());
        Ok(())
    }

    async fn reconcile_remount_watcher(
        &self,
        old_path: Option<&str>,
        new_path: &str,
    ) -> Result<(), String> {
        self.reconciled
            .lock()
            .unwrap()
            .push((old_path.map(|s| s.to_string()), new_path.to_string()));
        Ok(())
    }

    async fn trigger_remount_scan(&self, folder_path: &str) -> Result<(), String> {
        self.scanned.lock().unwrap().push(folder_path.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn unknown_volume_arrival_emits_removable_volume_detected() {
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let coordinator = RemovableVolumeCoordinator::new(pool, emitter.clone());

    let volume = VolumeInfo {
        volume_id: "{12345678-1234-1234-1234-123456789abc}".to_string(),
        mount_path: "E:\\".to_string(),
        display_name: "EOS_DIGITAL".to_string(),
    };

    coordinator
        .handle_event(VolumeEvent::Arrival(volume.clone()))
        .await
        .unwrap();

    let detected = emitter.detected.lock().unwrap().clone();
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0], volume);
}

#[tokio::test]
async fn known_volume_arrival_does_not_emit_removable_volume_detected() {
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone());

    // Register a watched folder and bind it to the volume
    let folder_id = crate::db::add_folder(&pool, "E:\\DCIM", None)
        .await
        .unwrap();
    let volume_id = "{12345678-1234-1234-1234-123456789abc}";
    crate::db::bind_removable_folder(&pool, folder_id, volume_id, "DCIM")
        .await
        .unwrap();

    let volume = VolumeInfo {
        volume_id: volume_id.to_string(),
        mount_path: "E:\\".to_string(),
        display_name: "EOS_DIGITAL".to_string(),
    };

    coordinator
        .handle_event(VolumeEvent::Arrival(volume))
        .await
        .unwrap();

    let detected = emitter.detected.lock().unwrap().clone();
    assert_eq!(detected.len(), 0);
}

#[cfg(target_os = "windows")]
#[test]
fn windows_fixed_drive_is_not_treated_as_removable() {
    let result = windows::get_volume_info_for_drive('C');
    assert!(result.is_none());
}

#[tokio::test]
async fn coordinator_arrival_and_removal_tracks_active_volumes() {
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let active_volumes = ActiveRemovableVolumes::new();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone())
        .with_active_volumes(active_volumes.clone());

    let volume = VolumeInfo {
        volume_id: "{vol-test-active}".to_string(),
        mount_path: "E:\\".to_string(),
        display_name: "ACTIVE_SD".to_string(),
    };

    assert!(active_volumes.get(&volume.volume_id).is_none());

    coordinator
        .handle_event(VolumeEvent::Arrival(volume.clone()))
        .await
        .unwrap();

    assert_eq!(active_volumes.get(&volume.volume_id), Some(volume.clone()));

    coordinator
        .handle_event(VolumeEvent::Removal(volume.mount_path.clone()))
        .await
        .unwrap();

    assert!(active_volumes.get(&volume.volume_id).is_none());
}

#[tokio::test]
async fn known_volume_arrival_with_changed_mount_remaps_path_reconciles_watcher_and_triggers_scan()
{
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone());

    let volume_id = "{12345678-1234-1234-1234-123456789abc}";
    let initial_path = if cfg!(windows) {
        "E:\\DCIM"
    } else {
        "/media/user/OLD/DCIM"
    };
    let new_mount = if cfg!(windows) {
        "F:\\"
    } else {
        "/media/user/NEW"
    };
    let expected_new_path = if cfg!(windows) {
        "F:\\DCIM"
    } else {
        "/media/user/NEW/DCIM"
    };

    let folder_id = crate::db::add_folder(&pool, initial_path, None)
        .await
        .unwrap();
    crate::db::bind_removable_folder(&pool, folder_id, volume_id, "DCIM")
        .await
        .unwrap();

    // Populate sync_state
    let asset_path = format!("{}/100EOS/IMG_0001.JPG", initial_path);
    crate::db::update_sync_state(
        &pool,
        &asset_path,
        "hash1",
        100,
        1000,
        "SYNCED",
        Some("rem1"),
    )
    .await
    .unwrap();

    let volume = VolumeInfo {
        volume_id: volume_id.to_string(),
        mount_path: new_mount.to_string(),
        display_name: "EOS_DIGITAL".to_string(),
    };

    coordinator
        .handle_event(VolumeEvent::Arrival(volume))
        .await
        .unwrap();

    // 1. User prompts are suppressed
    let detected = emitter.detected.lock().unwrap().clone();
    assert_eq!(detected.len(), 0);

    // 2. Watched folder is remapped in DB
    let folders = crate::db::get_folders(&pool).await.unwrap();
    let folder = folders.iter().find(|f| f.id == folder_id).unwrap();
    let norm_expected =
        crate::media::paths::normalize_folder_path(std::path::Path::new(expected_new_path))
            .to_string_lossy()
            .to_string();
    assert_eq!(folder.path, norm_expected);

    // 3. sync_state is remapped
    let old_state = sqlx::query("SELECT local_path FROM sync_state WHERE local_path = ?")
        .bind(&asset_path)
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(old_state.is_none());

    // 4. Watcher was reconciled with old path and new path
    let reconciled = emitter.reconciled.lock().unwrap().clone();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].0, Some(initial_path.to_string()));
    assert_eq!(reconciled[0].1, norm_expected);

    // 5. Media scan was triggered for the reconnected folder
    let scanned = emitter.scanned.lock().unwrap().clone();
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0], norm_expected);
}

#[tokio::test]
async fn known_volume_arrival_with_same_mount_reconciles_watcher_and_triggers_scan_without_remapping(
) {
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone());

    let volume_id = "{12345678-1234-1234-1234-123456789abc}";
    let mount_path = if cfg!(windows) {
        "E:\\"
    } else {
        "/media/user/SD"
    };
    let folder_path = if cfg!(windows) {
        "E:\\DCIM"
    } else {
        "/media/user/SD/DCIM"
    };

    let folder_id = crate::db::add_folder(&pool, folder_path, None)
        .await
        .unwrap();
    crate::db::bind_removable_folder(&pool, folder_id, volume_id, "DCIM")
        .await
        .unwrap();

    let volume = VolumeInfo {
        volume_id: volume_id.to_string(),
        mount_path: mount_path.to_string(),
        display_name: "EOS_DIGITAL".to_string(),
    };

    coordinator
        .handle_event(VolumeEvent::Arrival(volume))
        .await
        .unwrap();

    let detected = emitter.detected.lock().unwrap().clone();
    assert_eq!(detected.len(), 0);

    // Watcher reconciled with None for old_path (no unwatch needed since path is identical)
    let norm_path = crate::media::paths::normalize_folder_path(std::path::Path::new(folder_path))
        .to_string_lossy()
        .to_string();
    let reconciled = emitter.reconciled.lock().unwrap().clone();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].0, None);
    assert_eq!(reconciled[0].1, norm_path);

    // Scan triggered
    let scanned = emitter.scanned.lock().unwrap().clone();
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0], norm_path);
}
