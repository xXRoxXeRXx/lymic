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
    removed: Arc<Mutex<Vec<String>>>,
    reconciled: Arc<Mutex<Vec<(Option<String>, String)>>>,
    unwatched: Arc<Mutex<Vec<String>>>,
    scanned: Arc<Mutex<Vec<String>>>,
}

impl VolumeEventEmitter for MockEmitter {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String> {
        self.detected.lock().unwrap().push(volume.clone());
        Ok(())
    }

    fn emit_volume_removed(&self, id_or_path: &str) -> Result<(), String> {
        self.removed.lock().unwrap().push(id_or_path.to_string());
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

    async fn reconcile_folder_unavailable(&self, path: &str) -> Result<(), String> {
        self.unwatched.lock().unwrap().push(path.to_string());
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

#[tokio::test]
async fn volume_removal_clears_active_volumes_emits_removal_and_unwatches_bound_folders_without_deleting_rows(
) {
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let active_volumes = ActiveRemovableVolumes::new();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone())
        .with_active_volumes(active_volumes.clone());

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
    active_volumes.insert(volume.clone());

    // When card removal event arrives (by drive path or volume id):
    coordinator
        .handle_event(VolumeEvent::Removal(mount_path.to_string()))
        .await
        .unwrap();

    // 1. Clears active volume
    assert!(active_volumes.get(volume_id).is_none());

    // 2. Emits removal event with volume_id to clear pending prompt
    let removed = emitter.removed.lock().unwrap().clone();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0], volume_id);

    // 3. Reconciles bound folder as unavailable to unwatch cleanly
    let norm_path = crate::media::paths::normalize_folder_path(std::path::Path::new(folder_path))
        .to_string_lossy()
        .to_string();
    let unwatched = emitter.unwatched.lock().unwrap().clone();
    assert_eq!(unwatched.len(), 1);
    assert_eq!(unwatched[0], norm_path);

    // 4. Folder and binding rows are NOT deleted from SQLite
    let folders = crate::db::get_folders(&pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder_id);

    let binding = crate::db::get_removable_folder_binding(&pool, folder_id)
        .await
        .unwrap();
    assert!(binding.is_some());
}

#[tokio::test]
async fn manual_folder_addition_detects_removable_volume_and_persists_binding() {
    let pool = test_pool().await;
    let active_volumes = ActiveRemovableVolumes::new();

    let volume_id = "{99999999-1234-1234-1234-123456789abc}";
    let mount_path = if cfg!(windows) {
        "G:\\"
    } else {
        "/media/user/CARD"
    };
    let folder_path = if cfg!(windows) {
        "G:\\DCIM\\100MEDIA"
    } else {
        "/media/user/CARD/DCIM/100MEDIA"
    };

    active_volumes.insert(VolumeInfo {
        volume_id: volume_id.to_string(),
        mount_path: mount_path.to_string(),
        display_name: "SONY_SD".to_string(),
    });

    // Detect binding as add_folder does
    let binding = crate::commands::folders::detect_removable_binding_for_path(
        &active_volumes,
        std::path::Path::new(folder_path),
    );
    assert!(binding.is_some());
    let binding = binding.unwrap();
    assert_eq!(binding.volume_id, volume_id);
    assert_eq!(binding.relative_path, "DCIM/100MEDIA");

    // Add folder with this detected binding
    let folder_id = crate::db::add_folder(&pool, folder_path, None)
        .await
        .unwrap();
    crate::db::bind_removable_folder(&pool, folder_id, &binding.volume_id, &binding.relative_path)
        .await
        .unwrap();

    let stored_binding = crate::db::get_removable_folder_binding(&pool, folder_id)
        .await
        .unwrap();
    assert_eq!(
        stored_binding,
        Some(crate::db::RemovableFolderBinding {
            watched_folder_id: folder_id,
            volume_id: volume_id.to_string(),
            relative_path: "DCIM/100MEDIA".to_string(),
        })
    );
}

#[tokio::test]
async fn reconnecting_known_volume_resets_failed_queue_items_for_folder_to_pending() {
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
    let norm_folder = crate::media::paths::normalize_folder_path(std::path::Path::new(folder_path))
        .to_string_lossy()
        .to_string();

    let folder_id = crate::db::add_folder(&pool, folder_path, None)
        .await
        .unwrap();
    crate::db::bind_removable_folder(&pool, folder_id, volume_id, "DCIM")
        .await
        .unwrap();

    // Populate sync_queue with a failed item in this removable folder and an unrelated folder
    let card_asset = format!("{}/IMG_0001.JPG", norm_folder);
    let other_asset = if cfg!(windows) {
        "C:\\Local\\IMG_9999.JPG"
    } else {
        "/home/user/IMG_9999.JPG"
    };

    sqlx::query(
        "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status, upload_unit_id, is_primary, failure_reason) \
         VALUES (1, 1, ?, 100, 1000, 'FAILED', 'unit-1', 1, 'Card was removed mid-flight'), \
                (1, 2, ?, 200, 2000, 'FAILED', 'unit-2', 1, 'Server error')",
    )
    .bind(&card_asset)
    .bind(other_asset)
    .execute(&pool)
    .await
    .unwrap();

    crate::db::mark_sync_failed(&pool, &card_asset, 1000, 100, "Card was removed mid-flight")
        .await
        .unwrap();
    crate::db::mark_sync_failed(&pool, other_asset, 2000, 200, "Server error")
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

    // The card asset in sync_queue should be reset to PENDING and failure_reason cleared
    let card_queue_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM sync_queue WHERE local_path = ?")
            .bind(&card_asset)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(card_queue_status.0, "PENDING");
    assert!(card_queue_status.1.is_none());

    // The unrelated other asset should remain FAILED
    let other_queue_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM sync_queue WHERE local_path = ?")
            .bind(other_asset)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(other_queue_status.0, "FAILED");
    assert_eq!(other_queue_status.1.as_deref(), Some("Server error"));

    // Media scan was triggered for the folder
    let scanned = emitter.scanned.lock().unwrap().clone();
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0], norm_folder);
}

#[tokio::test]
async fn volume_removal_during_active_sync_transitions_inflight_to_failed_and_resumes_on_reconnect()
{
    let pool = test_pool().await;
    let emitter = MockEmitter::default();
    let active_volumes = ActiveRemovableVolumes::new();
    let coordinator = RemovableVolumeCoordinator::new(pool.clone(), emitter.clone())
        .with_active_volumes(active_volumes.clone());

    let volume_id = "{abcdef12-3456-7890-abcd-ef1234567890}";
    let mount_path = if cfg!(windows) {
        "E:\\"
    } else {
        "/media/user/SD"
    };
    let folder_path = if cfg!(windows) {
        "E:\\DCIM\\100NIKON"
    } else {
        "/media/user/SD/DCIM/100NIKON"
    };
    let norm_folder = crate::media::paths::normalize_folder_path(std::path::Path::new(folder_path))
        .to_string_lossy()
        .to_string();

    let folder_id = crate::db::add_folder(&pool, folder_path, None)
        .await
        .unwrap();
    crate::db::bind_removable_folder(&pool, folder_id, volume_id, "DCIM/100NIKON")
        .await
        .unwrap();

    let volume = VolumeInfo {
        volume_id: volume_id.to_string(),
        mount_path: mount_path.to_string(),
        display_name: "NIKON_CARD".to_string(),
    };
    active_volumes.insert(volume.clone());

    // 1. Asset is queued for active sync
    let asset_path = format!("{}/DSC_0001.JPG", norm_folder);
    sqlx::query(
        "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status, upload_unit_id, is_primary) \
         VALUES (1, 1, ?, 1024, 1700000000, 'PENDING', 'unit-nikon-1', 1)",
    )
    .bind(&asset_path)
    .execute(&pool)
    .await
    .unwrap();

    // 2. Active upload encounters card disconnection mid-flight
    let mid_flight_error = "Device disconnected or I/O error during upload stream";
    crate::db::mark_sync_failed(&pool, &asset_path, 1700000000, 1024, mid_flight_error)
        .await
        .unwrap();
    crate::db::finalize_queued_block(&pool, &[(asset_path.clone(), "FAILED".to_string())])
        .await
        .unwrap();

    // 3. Platform volume removal event arrives
    coordinator
        .handle_event(VolumeEvent::Removal(mount_path.to_string()))
        .await
        .unwrap();

    // Verification: Active volume removed, watcher unwatched, queue & state are in recoverable failure
    assert!(active_volumes.get(volume_id).is_none());
    let unwatched = emitter.unwatched.lock().unwrap().clone();
    assert_eq!(unwatched.len(), 1);
    assert_eq!(unwatched[0], norm_folder);

    let queue_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM sync_queue WHERE local_path = ?")
            .bind(&asset_path)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(queue_status.0, "FAILED");

    // 4. Card is reconnected
    coordinator
        .handle_event(VolumeEvent::Arrival(volume))
        .await
        .unwrap();

    // Verification: Failed queue item automatically resets to PENDING ready to resume
    let queue_status_reconnected: (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM sync_queue WHERE local_path = ?")
            .bind(&asset_path)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(queue_status_reconnected.0, "PENDING");
    assert!(queue_status_reconnected.1.is_none());

    // Rescan triggered
    let scanned = emitter.scanned.lock().unwrap().clone();
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0], norm_folder);
}
