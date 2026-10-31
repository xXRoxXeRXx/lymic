use crate::audit::{AuditEvent, Outcome, Severity};
use crate::immich::AlbumId;
use crate::media::paths::{
    canonicalize_folder_path, normalize_path_lexical, path_is_within_lexical,
};
use crate::removable::{ActiveRemovableVolumes, VolumeInfo};
use crate::watcher::WatchRegistration;
use serde_json::json;
use std::path::{Path, PathBuf};

mod internal {
    use crate::audit::AuditEvent;
    use crate::db::{self, RemovableFolderBinding, WatchedFolder};
    use crate::immich::AlbumId;
    use crate::watcher::{self, WatchRegistration, WatcherState};

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
        async fn remove_folder(&self, folder_id: i64) -> Result<(), String>;
    }

    pub(crate) trait FolderWatcher {
        async fn watch_available(&self, path: &str) -> Result<WatchRegistration, String>;
        async fn unwatch(&self, path: &str) -> Result<(), String>;
    }

    pub(crate) trait RegistrationEffects {
        fn enable_sync(&self);
        fn trigger_folder_added_sync(&self, folder: WatchedFolder);
        fn log_watcher_deferred(&self, path: &str);
        fn log_watcher_undo_failed(&self, path: &str, error: &str);
        fn record_audit(&self, event: AuditEvent);
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

        async fn remove_folder(&self, folder_id: i64) -> Result<(), String> {
            db::remove_folder(self.pool, folder_id)
                .await
                .map_err(|error| error.to_string())
        }
    }

    pub(crate) struct NotifyFolderWatcher<'a> {
        pub(crate) watcher: &'a tokio::sync::Mutex<WatcherState>,
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

    pub(crate) struct TauriRegistrationEffects<'a> {
        pub(crate) app: &'a tauri::AppHandle,
        pub(crate) pool: &'a sqlx::SqlitePool,
    }

    impl RegistrationEffects for TauriRegistrationEffects<'_> {
        fn enable_sync(&self) {
            crate::app::tray::set_sync_enabled(self.app, true);
        }

        fn trigger_folder_added_sync(&self, folder: WatchedFolder) {
            let pool_inner = self.pool.clone();
            let app_for_scan = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let scan_result =
                    match crate::sync::scan_watched_folders_for_media(vec![folder.clone()]).await {
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
                crate::sync::admit_and_run(
                    app_for_scan,
                    pool_inner,
                    scan_result,
                    crate::sync::SyncSource::FolderAdded,
                )
                .await;
            });
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

        fn record_audit(&self, event: AuditEvent) {
            crate::audit::audit_event(self.app, event);
        }
    }
}

use internal::{
    FolderPersistence, FolderWatcher, NotifyFolderWatcher, RegistrationEffects,
    SqliteFolderPersistence, TauriRegistrationEffects,
};

fn sanitize_target_album_id(target_album_id: Option<String>) -> Option<AlbumId> {
    target_album_id.and_then(|id| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(AlbumId::new(trimmed))
        }
    })
}

fn check_folder_conflict(
    candidate_path: &Path,
    existing_paths: &[PathBuf],
) -> Result<PathBuf, String> {
    let normalized_path = canonicalize_folder_path(candidate_path);
    let conflict = existing_paths.iter().find_map(|existing_path| {
        let normalized_existing = normalize_path_lexical(existing_path);
        let candidate_within = path_is_within_lexical(&normalized_path, &normalized_existing);
        let existing_within = path_is_within_lexical(&normalized_existing, &normalized_path);
        if candidate_within || existing_within {
            Some((normalized_existing, candidate_within && existing_within))
        } else {
            None
        }
    });

    if let Some((existing_path, is_duplicate)) = conflict {
        let message = if is_duplicate {
            format!(
                "Folder '{}' is already being watched.",
                normalized_path.display()
            )
        } else {
            format!(
                "Folder '{}' overlaps with already watched folder '{}'. Remove one of the folders before adding it.",
                normalized_path.display(),
                existing_path.display()
            )
        };
        return Err(message);
    }

    Ok(normalized_path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovableVolumeBinding {
    volume_id: String,
    relative_path: String,
    reject_if_already_bound: bool,
}

impl RemovableVolumeBinding {
    pub fn new(volume_id: impl Into<String>, relative_path: impl Into<String>) -> Self {
        Self {
            volume_id: volume_id.into(),
            relative_path: relative_path.into(),
            reject_if_already_bound: false,
        }
    }

    pub fn volume_id(&self) -> &str {
        &self.volume_id
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisterFolderParams {
    path: String,
    target_album_id: Option<AlbumId>,
    removable_binding: Option<RemovableVolumeBinding>,
}

impl RegisterFolderParams {
    pub fn new(path: impl Into<String>, target_album_id: Option<String>) -> Self {
        Self {
            path: path.into(),
            target_album_id: sanitize_target_album_id(target_album_id),
            removable_binding: None,
        }
    }

    pub fn from_local_path(
        path: impl Into<String>,
        target_album_id: Option<String>,
        active_volumes: Option<&ActiveRemovableVolumes>,
    ) -> Self {
        let path = path.into();
        let removable_binding = active_volumes
            .and_then(|volumes| detect_removable_binding_for_path(volumes, Path::new(&path)));
        Self {
            path,
            target_album_id: sanitize_target_album_id(target_album_id),
            removable_binding,
        }
    }

    pub fn from_removable_acceptance(
        active_volumes: &ActiveRemovableVolumes,
        volume_id: String,
        path: String,
        target_album_id: Option<String>,
    ) -> Result<Self, String> {
        let volume = active_volumes.get(&volume_id);
        let relative_path =
            validate_removable_volume_containment(volume.as_ref(), &volume_id, Path::new(&path))?;

        Ok(
            Self::new(path, target_album_id).with_removable_binding(RemovableVolumeBinding {
                volume_id,
                relative_path,
                reject_if_already_bound: true,
            }),
        )
    }

    pub fn with_removable_binding(mut self, binding: RemovableVolumeBinding) -> Self {
        self.removable_binding = Some(binding);
        self
    }

    #[cfg(test)]
    pub fn removable_binding(&self) -> Option<&RemovableVolumeBinding> {
        self.removable_binding.as_ref()
    }
}

pub(crate) struct WatchedFolderRegistration<P, W, E> {
    persistence: P,
    watcher: W,
    effects: E,
}

impl<'a>
    WatchedFolderRegistration<
        SqliteFolderPersistence<'a>,
        NotifyFolderWatcher<'a>,
        TauriRegistrationEffects<'a>,
    >
{
    pub(crate) fn for_tauri(
        app: &'a tauri::AppHandle,
        pool: &'a sqlx::SqlitePool,
        watcher: &'a tokio::sync::Mutex<crate::watcher::WatcherState>,
    ) -> Self {
        Self::new(
            SqliteFolderPersistence { pool },
            NotifyFolderWatcher { watcher },
            TauriRegistrationEffects { app, pool },
        )
    }
}

impl<P, W, E> WatchedFolderRegistration<P, W, E>
where
    P: FolderPersistence,
    W: FolderWatcher,
    E: RegistrationEffects,
{
    pub(crate) fn new(persistence: P, watcher: W, effects: E) -> Self {
        Self {
            persistence,
            watcher,
            effects,
        }
    }

    pub(crate) async fn register(&self, input: RegisterFolderParams) -> Result<i64, String> {
        if let Some(ref removable) = input.removable_binding {
            if removable.reject_if_already_bound {
                let existing = self
                    .persistence
                    .list_removable_bindings_by_volume(removable.volume_id())
                    .await?;
                if !existing.is_empty() {
                    return Err(format!(
                        "Removable volume '{}' is already bound to a watched folder.",
                        removable.volume_id()
                    ));
                }
            }
        }

        let folders = self.persistence.list_folders().await?;
        let path_for_normalization = PathBuf::from(&input.path);
        let existing_paths = folders
            .into_iter()
            .map(|folder| PathBuf::from(folder.path))
            .collect::<Vec<_>>();

        let normalized_path = tokio::task::spawn_blocking(move || {
            check_folder_conflict(&path_for_normalization, &existing_paths)
        })
        .await
        .map_err(|error| format!("Could not normalize folder '{}': {}", input.path, error))??;

        let path_string = normalized_path.to_string_lossy().to_string();
        let path_for_check = normalized_path.clone();

        let is_available = match tokio::task::spawn_blocking(move || {
            std::fs::metadata(path_for_check).map(|metadata| metadata.is_dir())
        })
        .await
        {
            Ok(Ok(true)) => true,
            Ok(Ok(false)) => return Err(format!("Path '{}' is not a directory.", path_string)),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
            Ok(Err(error)) => {
                return Err(format!(
                    "Could not access folder '{}': {}",
                    path_string, error
                ))
            }
            Err(error) => {
                return Err(format!(
                    "Could not check folder '{}': {}",
                    path_string, error
                ))
            }
        };

        let registration = if is_available {
            self.watcher.watch_available(&path_string).await?
        } else {
            WatchRegistration::PathUnavailable
        };

        let id = match self
            .persistence
            .add_folder(&path_string, input.target_album_id.as_ref())
            .await
        {
            Ok(id) => id,
            Err(error) => {
                if registration == WatchRegistration::Registered {
                    if let Err(unwatch_error) = self.watcher.unwatch(&path_string).await {
                        self.effects
                            .log_watcher_undo_failed(&path_string, &unwatch_error);
                    }
                }
                return Err(error);
            }
        };

        if let Some(removable) = input.removable_binding {
            if let Err(error) = self
                .persistence
                .bind_removable_folder(id, removable.volume_id(), removable.relative_path())
                .await
            {
                let _ = self.persistence.remove_folder(id).await;
                if registration == WatchRegistration::Registered {
                    let _ = self.watcher.unwatch(&path_string).await;
                }
                return Err(error);
            }
        }

        // The persisted folder is the source of truth, even when its watcher is deferred.
        self.effects.enable_sync();

        if registration == WatchRegistration::PathUnavailable {
            self.effects.log_watcher_deferred(&path_string);
        }

        // An offline folder will be scanned when the deferred-watch task registers it.
        if is_available {
            let folder = self
                .persistence
                .list_folders()
                .await?
                .into_iter()
                .find(|folder| folder.id == id)
                .ok_or_else(|| "Added Watched Folder was not found.".to_string())?;
            self.effects.trigger_folder_added_sync(folder);
        }

        self.effects.record_audit(AuditEvent::new(
            AuditEvent::operation_id(),
            "folder.added",
            Outcome::Success,
            Severity::Info,
            None,
            "command",
            "Watched folder added",
            json!({
                "folder_id": id,
                "local_path": path_string,
                "target_album_id": input.target_album_id.as_ref().map(AlbumId::as_str),
            }),
        ));

        Ok(id)
    }
}

fn detect_removable_binding_for_path(
    active_volumes: &ActiveRemovableVolumes,
    path: &Path,
) -> Option<RemovableVolumeBinding> {
    let normalized_path = normalize_path_lexical(path);
    let volumes = active_volumes.list();
    let mut matching_volumes: Vec<(VolumeInfo, String)> = volumes
        .into_iter()
        .filter_map(|vol| {
            let relative_path =
                relative_path_within_mount(Path::new(&vol.mount_path), &normalized_path)?;
            Some((vol, relative_path))
        })
        .collect();

    matching_volumes.sort_by_key(|(vol, _)| std::cmp::Reverse(vol.mount_path.len()));

    matching_volumes
        .into_iter()
        .next()
        .map(|(best_volume, relative_path)| {
            RemovableVolumeBinding::new(best_volume.volume_id, relative_path)
        })
}

fn relative_path_within_mount(mount_path: &Path, normalized_folder: &Path) -> Option<String> {
    let normalized_mount = normalize_path_lexical(mount_path);
    if !path_is_within_lexical(normalized_folder, &normalized_mount) {
        return None;
    }

    let mount_count = normalized_mount.components().count();
    let folder_components: Vec<_> = normalized_folder.components().collect();

    if folder_components.len() <= mount_count {
        return Some(String::new());
    }

    let parts = folder_components[mount_count..]
        .iter()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    Some(parts.join("/"))
}

fn validate_removable_volume_containment(
    volume: Option<&VolumeInfo>,
    volume_id: &str,
    path: &Path,
) -> Result<String, String> {
    let volume =
        volume.ok_or_else(|| format!("Removable volume '{}' is not connected.", volume_id))?;

    let normalized_path = normalize_path_lexical(path);
    relative_path_within_mount(Path::new(&volume.mount_path), &normalized_path).ok_or_else(|| {
        format!(
            "Path '{}' does not reside within removable volume '{}' (mounted at '{}').",
            path.display(),
            volume_id,
            volume.mount_path
        )
    })
}

#[cfg(test)]
mod tests;
