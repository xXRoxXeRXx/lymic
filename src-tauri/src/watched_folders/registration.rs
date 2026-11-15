use super::seams::{FolderEffects, FolderPersistence, FolderWatcher};
use crate::audit::{AuditEvent, Outcome, Severity};
use crate::immich::AlbumId;
use crate::media::paths::{
    canonicalize_folder_path, normalize_path_lexical, path_is_within_lexical,
};
use crate::removable::{ActiveRemovableVolumes, VolumeInfo};
use crate::watcher::WatchRegistration;
use serde_json::json;
use std::path::{Path, PathBuf};

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

pub(crate) async fn register_folder<P, W, E>(
    persistence: &P,
    watcher: &W,
    effects: &E,
    input: RegisterFolderParams,
) -> Result<i64, String>
where
    P: FolderPersistence,
    W: FolderWatcher,
    E: FolderEffects,
{
    if let Some(ref removable) = input.removable_binding {
        if removable.reject_if_already_bound {
            let existing = persistence
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

    let folders = persistence.list_folders().await?;
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
        watcher.watch_available(&path_string).await?
    } else {
        WatchRegistration::PathUnavailable
    };

    let id = match persistence
        .add_folder(&path_string, input.target_album_id.as_ref())
        .await
    {
        Ok(id) => id,
        Err(error) => {
            if registration == WatchRegistration::Registered {
                if let Err(unwatch_error) = watcher.unwatch(&path_string).await {
                    effects.log_watcher_undo_failed(&path_string, &unwatch_error);
                }
            }
            return Err(error);
        }
    };

    if let Some(removable) = input.removable_binding {
        if let Err(error) = persistence
            .bind_removable_folder(id, removable.volume_id(), removable.relative_path())
            .await
        {
            let _ = persistence.remove_folder(id).await;
            if registration == WatchRegistration::Registered {
                let _ = watcher.unwatch(&path_string).await;
            }
            return Err(error);
        }
    }

    // The persisted folder is the source of truth, even when its watcher is deferred.
    effects.set_sync_enabled(true);

    if registration == WatchRegistration::PathUnavailable {
        effects.log_watcher_deferred(&path_string);
    }

    // An offline folder will be scanned when the deferred-watch task registers it.
    if is_available {
        let folder = persistence
            .list_folders()
            .await?
            .into_iter()
            .find(|folder| folder.id == id)
            .ok_or_else(|| "Added watched folder was not found.".to_string())?;
        effects.trigger_folder_scan(folder, crate::sync::SyncSource::FolderAdded);
    }

    effects.record_audit(AuditEvent::new(
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
