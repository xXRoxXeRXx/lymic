use super::model::{VolumeEvent, VolumeInfo};
use sqlx::SqlitePool;

pub trait VolumeEventEmitter: Send + Sync {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String>;
}

pub struct RemovableVolumeCoordinator<E: VolumeEventEmitter> {
    pool: SqlitePool,
    emitter: E,
}

impl<E: VolumeEventEmitter> RemovableVolumeCoordinator<E> {
    pub fn new(pool: SqlitePool, emitter: E) -> Self {
        Self { pool, emitter }
    }

    pub async fn handle_event(&self, event: VolumeEvent) -> Result<(), String> {
        match event {
            VolumeEvent::Arrival(volume) => {
                let bindings =
                    crate::db::get_removable_folders_by_volume(&self.pool, &volume.volume_id)
                        .await
                        .map_err(|e| e.to_string())?;

                if bindings.is_empty() {
                    self.emitter.emit_volume_detected(&volume)?;
                }
                Ok(())
            }
            VolumeEvent::Removal(_volume_id) => {
                // In Issue #26, volume removal unregisters watchers and transitions queue items
                Ok(())
            }
        }
    }
}
