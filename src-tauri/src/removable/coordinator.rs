use super::model::{ActiveRemovableVolumes, VolumeEvent, VolumeInfo};
use sqlx::SqlitePool;

pub trait VolumeEventEmitter: Send + Sync {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String>;
}

pub struct RemovableVolumeCoordinator<E: VolumeEventEmitter> {
    pool: SqlitePool,
    emitter: E,
    active_volumes: Option<ActiveRemovableVolumes>,
}

impl<E: VolumeEventEmitter> RemovableVolumeCoordinator<E> {
    pub fn new(pool: SqlitePool, emitter: E) -> Self {
        Self {
            pool,
            emitter,
            active_volumes: None,
        }
    }

    pub fn with_active_volumes(mut self, active_volumes: ActiveRemovableVolumes) -> Self {
        self.active_volumes = Some(active_volumes);
        self
    }

    pub async fn handle_event(&self, event: VolumeEvent) -> Result<(), String> {
        match event {
            VolumeEvent::Arrival(volume) => {
                if let Some(ref active) = self.active_volumes {
                    active.insert(volume.clone());
                }

                let bindings =
                    crate::db::get_removable_folders_by_volume(&self.pool, &volume.volume_id)
                        .await
                        .map_err(|e| e.to_string())?;

                if bindings.is_empty() {
                    self.emitter.emit_volume_detected(&volume)?;
                }
                Ok(())
            }
            VolumeEvent::Removal(id_or_path) => {
                if let Some(ref active) = self.active_volumes {
                    active.remove(&id_or_path);
                }
                // In Issue #26, volume removal unregisters watchers and transitions queue items
                Ok(())
            }
        }
    }
}
