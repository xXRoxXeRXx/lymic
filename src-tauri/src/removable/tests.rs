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
}

impl VolumeEventEmitter for MockEmitter {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String> {
        self.detected.lock().unwrap().push(volume.clone());
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
