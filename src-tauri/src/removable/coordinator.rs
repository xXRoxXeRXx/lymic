use super::model::{ActiveRemovableVolumes, VolumeEvent, VolumeInfo, VolumeRemoval};
use crate::watched_folders::{
    FolderScanTrigger, FolderWatcher, NotifyFolderWatcher, TauriFolderScanTrigger,
};
use sqlx::SqlitePool;

pub(crate) trait VolumeEventEmitter: Send + Sync {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String>;
    fn emit_volume_remounted(&self, volume: &VolumeInfo) -> Result<(), String>;
    fn emit_volume_removed(&self, volume_id: &str) -> Result<(), String>;
}

pub(crate) struct RemovableVolumeCoordinator<E, W, T> {
    pool: SqlitePool,
    emitter: E,
    watcher: W,
    scan_trigger: T,
    active_volumes: Option<ActiveRemovableVolumes>,
}

impl<'a>
    RemovableVolumeCoordinator<
        tauri::AppHandle,
        NotifyFolderWatcher<'a>,
        TauriFolderScanTrigger<'a>,
    >
{
    pub(crate) fn for_tauri(app: &'a tauri::AppHandle, pool: &'a SqlitePool) -> Self {
        Self::new(
            pool.clone(),
            app.clone(),
            NotifyFolderWatcher::for_tauri(app),
            TauriFolderScanTrigger { app, pool },
        )
    }
}

impl<E, W, T> RemovableVolumeCoordinator<E, W, T>
where
    E: VolumeEventEmitter,
    W: FolderWatcher,
    T: FolderScanTrigger,
{
    pub(crate) fn new(pool: SqlitePool, emitter: E, watcher: W, scan_trigger: T) -> Self {
        Self {
            pool,
            emitter,
            watcher,
            scan_trigger,
            active_volumes: None,
        }
    }

    pub(crate) fn with_active_volumes(mut self, active_volumes: ActiveRemovableVolumes) -> Self {
        self.active_volumes = Some(active_volumes);
        self
    }

    pub(crate) async fn handle_event(&self, event: VolumeEvent) -> Result<(), String> {
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
                                crate::media::paths::normalize_path_lexical(&candidate_path);
                            let norm_new_str = norm_new.to_string_lossy().to_string();

                            let norm_current = crate::media::paths::normalize_path_lexical(
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

                                let _ = self.watcher.unwatch(&folder.path).await;
                            }
                            self.watcher.watch_available(&norm_new_str).await?;

                            crate::db::reset_failed_queue_items_for_folder(
                                &self.pool,
                                &norm_new_str,
                            )
                            .await
                            .map_err(|e| e.to_string())?;

                            let mut remounted_folder = folder.clone();
                            remounted_folder.path = norm_new_str;
                            self.scan_trigger.trigger_folder_scan(
                                remounted_folder,
                                crate::sync::SyncSource::RemovableMediaReconnect,
                            );

                            // Event delivery must not prevent the path remap, watcher
                            // reconciliation, or remount scan from completing. At this point
                            // any error is reported by the background event consumer.
                            self.emitter.emit_volume_remounted(&volume)?;
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
                        self.watcher.unwatch(&folder.path).await?;
                    }
                }

                Ok(())
            }
        }
    }
}
