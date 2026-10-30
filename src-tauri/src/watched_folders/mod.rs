use crate::audit::{AuditEvent, Outcome, Severity};
use crate::db;
use crate::immich::AlbumId;
use crate::media::paths::{normalize_folder_path, path_is_within};
use crate::removable::{ActiveRemovableVolumes, VolumeInfo};
use crate::watcher::{self, WatchRegistration, WatcherState};
use serde_json::json;
use std::path::{Path, PathBuf};

mod internal {
    use crate::audit::AuditEvent;
    use std::path::PathBuf;

    pub(crate) trait RegistrationEffects {
        fn enable_sync(&self);
        fn trigger_folder_added_sync(&self, path: String);
        fn log_watcher_deferred(&self, path: &str);
        fn log_watcher_undo_failed(&self, path: &str, error: &str);
        fn record_audit(&self, event: AuditEvent);
    }

    pub(crate) struct TauriRegistrationEffects<'a> {
        pub(crate) app: &'a tauri::AppHandle,
        pub(crate) pool: &'a sqlx::SqlitePool,
    }

    impl RegistrationEffects for TauriRegistrationEffects<'_> {
        fn enable_sync(&self) {
            crate::app::tray::set_sync_enabled(self.app, true);
        }

        fn trigger_folder_added_sync(&self, path: String) {
            let pool_inner = self.pool.clone();
            let app_for_scan = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let scan_result =
                    match crate::sync::scan_folders_for_media(vec![PathBuf::from(path.clone())])
                        .await
                    {
                        Ok(result) => result,
                        Err(error) => {
                            crate::app::events::log_to_ui_folder_error(
                                &app_for_scan,
                                "ERROR",
                                &format!("Folder scanning failed for '{path}': {error}"),
                                "log_folder_scan_failed",
                                &path,
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

use internal::{RegistrationEffects, TauriRegistrationEffects};

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
    let normalized_path = normalize_folder_path(candidate_path);
    let conflict = existing_paths.iter().find_map(|existing_path| {
        let normalized_existing = normalize_folder_path(existing_path);
        let is_duplicate = path_is_within(&normalized_path, &normalized_existing)
            && path_is_within(&normalized_existing, &normalized_path);
        if is_duplicate
            || path_is_within(&normalized_path, &normalized_existing)
            || path_is_within(&normalized_existing, &normalized_path)
        {
            Some((normalized_existing, is_duplicate))
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
pub(crate) struct RemovableBindingInput {
    pub volume_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisterFolderParams {
    pub path: String,
    pub target_album_id: Option<AlbumId>,
    pub removable_binding: Option<RemovableBindingInput>,
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

    pub async fn from_removable_acceptance(
        pool: &sqlx::SqlitePool,
        active_volumes: &ActiveRemovableVolumes,
        volume_id: String,
        path: String,
        target_album_id: Option<String>,
    ) -> Result<Self, String> {
        let volume = active_volumes.get(&volume_id);
        let existing_bindings = db::get_removable_folders_by_volume(pool, &volume_id)
            .await
            .map_err(|error| error.to_string())?;

        let relative_path = validate_removable_folder_acceptance(
            volume.as_ref(),
            &volume_id,
            Path::new(&path),
            &existing_bindings,
        )?;

        Ok(Self::new(path, target_album_id).with_removable_binding(volume_id, relative_path))
    }

    pub fn with_removable_binding(
        mut self,
        volume_id: impl Into<String>,
        relative_path: impl Into<String>,
    ) -> Self {
        self.removable_binding = Some(RemovableBindingInput {
            volume_id: volume_id.into(),
            relative_path: relative_path.into(),
        });
        self
    }
}

pub(crate) struct WatchedFolderRegistration<'a, E = TauriRegistrationEffects<'a>> {
    pool: &'a sqlx::SqlitePool,
    watcher: &'a tokio::sync::Mutex<WatcherState>,
    effects: E,
}

impl<'a> WatchedFolderRegistration<'a, TauriRegistrationEffects<'a>> {
    pub(crate) fn new(
        app: &'a tauri::AppHandle,
        pool: &'a sqlx::SqlitePool,
        watcher: &'a tokio::sync::Mutex<WatcherState>,
    ) -> Self {
        Self {
            pool,
            watcher,
            effects: TauriRegistrationEffects { app, pool },
        }
    }
}

impl<'a, E: RegistrationEffects> WatchedFolderRegistration<'a, E> {
    #[cfg(test)]
    pub(crate) fn for_test(
        pool: &'a sqlx::SqlitePool,
        watcher: &'a tokio::sync::Mutex<WatcherState>,
        effects: E,
    ) -> Self {
        Self {
            pool,
            watcher,
            effects,
        }
    }

    pub(crate) async fn register(&self, input: RegisterFolderParams) -> Result<i64, String> {
        let folders = db::get_folders(self.pool)
            .await
            .map_err(|error| error.to_string())?;
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
            let mut w = self.watcher.lock().await;
            w.reconcile_path(&path_string, true)
                .map_err(|error| format!("Could not watch '{}': {}", path_string, error))?
        } else {
            WatchRegistration::PathUnavailable
        };

        let id = match db::add_folder(self.pool, &path_string, input.target_album_id.as_ref()).await
        {
            Ok(id) => id,
            Err(error) => {
                self.undo_watcher_registration(&registration, &path_string)
                    .await;
                return Err(error.to_string());
            }
        };

        if let Some(removable) = input.removable_binding {
            if let Err(error) = db::bind_removable_folder(
                self.pool,
                id,
                &removable.volume_id,
                &removable.relative_path,
            )
            .await
            {
                let _ = db::remove_folder(self.pool, id).await;
                self.undo_watcher_registration(&registration, &path_string)
                    .await;
                return Err(error.to_string());
            }
        }

        // The persisted folder is the source of truth, even when its watcher is deferred.
        self.effects.enable_sync();

        if registration == WatchRegistration::PathUnavailable {
            self.effects.log_watcher_deferred(&path_string);
        }

        // An offline folder will be scanned when the deferred-watch task registers it.
        if is_available {
            self.effects.trigger_folder_added_sync(path_string.clone());
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

    async fn undo_watcher_registration(&self, registration: &WatchRegistration, path: &str) {
        if *registration == WatchRegistration::Registered {
            let mut w = self.watcher.lock().await;
            if let Err(unwatch_error) = watcher::unwatch_path(&mut w, path) {
                self.effects
                    .log_watcher_undo_failed(path, &unwatch_error.to_string());
            }
        }
    }
}

fn detect_removable_binding_for_path(
    active_volumes: &ActiveRemovableVolumes,
    path: &Path,
) -> Option<RemovableBindingInput> {
    let normalized_path = normalize_folder_path(path);
    let volumes = active_volumes.0.lock().ok()?;
    let mut matching_volumes: Vec<(&VolumeInfo, String)> = volumes
        .values()
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
        .map(|(best_volume, relative_path)| RemovableBindingInput {
            volume_id: best_volume.volume_id.clone(),
            relative_path,
        })
}

fn relative_path_within_mount(mount_path: &Path, normalized_folder: &Path) -> Option<String> {
    let normalized_mount = normalize_folder_path(mount_path);
    if !path_is_within(normalized_folder, &normalized_mount) {
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

fn validate_removable_folder_acceptance(
    volume: Option<&VolumeInfo>,
    volume_id: &str,
    path: &Path,
    existing_bindings: &[db::RemovableFolderBinding],
) -> Result<String, String> {
    let volume =
        volume.ok_or_else(|| format!("Removable volume '{}' is not connected.", volume_id))?;

    if !existing_bindings.is_empty() {
        return Err(format!(
            "Removable volume '{}' is already bound to a watched folder.",
            volume_id
        ));
    }

    let normalized_path = normalize_folder_path(path);
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
