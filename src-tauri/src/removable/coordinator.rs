use super::model::{ActiveRemovableVolumes, VolumeEvent, VolumeInfo, VolumeRemoval};
use sqlx::SqlitePool;

pub trait VolumeEventEmitter: Send + Sync {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String>;
    fn emit_volume_removed(&self, volume_id: &str) -> Result<(), String>;

    fn reconcile_remount_watcher(
        &self,
        old_path: Option<&str>,
        new_path: &str,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;

    fn reconcile_folder_unavailable(
        &self,
        path: &str,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;

    fn trigger_remount_scan(
        &self,
        folder_path: &str,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
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
                } else {
                    let folders = crate::db::get_folders(&self.pool)
                        .await
                        .map_err(|e| e.to_string())?;

                    for binding in bindings {
                        if let Some(folder) =
                            folders.iter().find(|f| f.id == binding.watched_folder_id)
                        {
                            let candidate_path = if binding.relative_path.is_empty() {
                                std::path::PathBuf::from(&volume.mount_path)
                            } else {
                                std::path::Path::new(&volume.mount_path)
                                    .join(&binding.relative_path)
                            };
                            let norm_new =
                                crate::media::paths::normalize_folder_path(&candidate_path);
                            let norm_new_str = norm_new.to_string_lossy().to_string();

                            let norm_current = crate::media::paths::normalize_folder_path(
                                std::path::Path::new(&folder.path),
                            );
                            let norm_current_str = norm_current.to_string_lossy().to_string();

                            let path_changed = norm_current_str != norm_new_str;

                            if path_changed {
                                crate::db::remap_watched_folder_path(
                                    &self.pool,
                                    folder.id,
                                    &folder.path,
                                    &norm_new_str,
                                )
                                .await
                                .map_err(|e| e.to_string())?;

                                self.emitter
                                    .reconcile_remount_watcher(Some(&folder.path), &norm_new_str)
                                    .await?;
                            } else {
                                self.emitter
                                    .reconcile_remount_watcher(None, &norm_new_str)
                                    .await?;
                            }

                            crate::db::reset_failed_queue_items_for_folder(
                                &self.pool,
                                &norm_new_str,
                            )
                            .await
                            .map_err(|e| e.to_string())?;

                            self.emitter.trigger_remount_scan(&norm_new_str).await?;
                        }
                    }
                }
                Ok(())
            }
            VolumeEvent::Removal(removal) => {
                let volume_info = match &removal {
                    VolumeRemoval::VolumeId(id) => {
                        self.active_volumes.as_ref().and_then(|a| a.get(id))
                    }
                    VolumeRemoval::MountPath(path) => self
                        .active_volumes
                        .as_ref()
                        .and_then(|a| a.find_by_mount_path(path)),
                };
                let volume_id = match (&volume_info, &removal) {
                    (Some(info), _) => info.volume_id.clone(),
                    (None, VolumeRemoval::VolumeId(id)) => id.clone(),
                    (None, VolumeRemoval::MountPath(path)) => path.clone(),
                };

                if let Some(ref active) = self.active_volumes {
                    match &removal {
                        VolumeRemoval::VolumeId(id) => active.remove(id),
                        VolumeRemoval::MountPath(path) => active.remove_by_mount_path(path),
                    }
                }

                self.emitter.emit_volume_removed(&volume_id)?;

                let bindings = crate::db::get_removable_folders_by_volume(&self.pool, &volume_id)
                    .await
                    .map_err(|e| e.to_string())?;

                let folders = crate::db::get_folders(&self.pool)
                    .await
                    .map_err(|e| e.to_string())?;

                for binding in bindings {
                    if let Some(folder) = folders.iter().find(|f| f.id == binding.watched_folder_id)
                    {
                        self.emitter
                            .reconcile_folder_unavailable(&folder.path)
                            .await?;
                    }
                }

                Ok(())
            }
        }
    }
}
