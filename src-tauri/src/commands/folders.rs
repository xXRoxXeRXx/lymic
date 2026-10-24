use crate::app::events::{log_to_ui_error, log_to_ui_folder, log_to_ui_folder_error};
use crate::app::tray::set_sync_enabled;
use crate::audit::audit_event;
use crate::media::paths::{normalize_folder_path, path_is_within};
use crate::{
    db,
    sync::{
        scan_folders_for_media, scan_watched_folders_for_media, sync_scan_result_if_authenticated,
        SyncCoordinator,
    },
    watcher,
};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveFolderResult {
    requires_sync_cancellation: bool,
}

fn removal_requires_confirmation(confirm_cancellation: Option<bool>) -> bool {
    !confirm_cancellation.unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovableBindingInput {
    pub volume_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisterFolderParams {
    pub path: String,
    pub target_album_id: Option<String>,
    pub removable_binding: Option<RemovableBindingInput>,
}

impl RegisterFolderParams {
    pub fn new(path: impl Into<String>, target_album_id: Option<String>) -> Self {
        Self {
            path: path.into(),
            target_album_id,
            removable_binding: None,
        }
    }

    #[allow(dead_code)]
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

pub(crate) fn sanitize_target_album_id(target_album_id: Option<String>) -> Option<String> {
    target_album_id.and_then(|id| {
        let trimmed = id.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub(crate) fn normalize_exclusion_patterns(patterns: Vec<String>) -> Result<Vec<String>, String> {
    crate::media::admission::normalize_patterns(patterns)
}

pub(crate) fn check_folder_conflict(
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

pub(crate) async fn register_watched_folder(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    watcher: &tokio::sync::Mutex<watcher::WatcherState>,
    params: RegisterFolderParams,
) -> Result<i64, String> {
    let folders = db::get_folders(pool)
        .await
        .map_err(|error| error.to_string())?;
    let path_for_normalization = PathBuf::from(&params.path);
    let existing_paths = folders
        .into_iter()
        .map(|folder| PathBuf::from(folder.path))
        .collect::<Vec<_>>();

    let normalized_path = tokio::task::spawn_blocking(move || {
        check_folder_conflict(&path_for_normalization, &existing_paths)
    })
    .await
    .map_err(|error| format!("Could not normalize folder '{}': {}", params.path, error))??;

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
        let mut w = watcher.lock().await;
        w.reconcile_path(&path_string, true)
            .map_err(|error| format!("Could not watch '{}': {}", path_string, error))?
    } else {
        watcher::WatchRegistration::PathUnavailable
    };

    let target_album_id = sanitize_target_album_id(params.target_album_id);
    let album_id = target_album_id.as_deref().map(crate::immich::AlbumId::new);

    let id = match db::add_folder(pool, &path_string, album_id.as_ref()).await {
        Ok(id) => id,
        Err(error) => {
            if registration == watcher::WatchRegistration::Registered {
                let mut w = watcher.lock().await;
                if let Err(unwatch_error) = watcher::unwatch_path(&mut w, &path_string) {
                    log_to_ui_folder_error(
                        app,
                        "ERROR",
                        &format!(
                            "Could not undo watcher registration for '{}': {}",
                            path_string, unwatch_error
                        ),
                        "log_watcher_undo_failed",
                        &path_string,
                        &unwatch_error.to_string(),
                    );
                }
            }
            return Err(error.to_string());
        }
    };

    if let Some(removable) = params.removable_binding {
        if let Err(error) =
            db::bind_removable_folder(pool, id, &removable.volume_id, &removable.relative_path)
                .await
        {
            let _ = db::remove_folder(pool, id).await;
            if registration == watcher::WatchRegistration::Registered {
                let mut w = watcher.lock().await;
                let _ = watcher::unwatch_path(&mut w, &path_string);
            }
            return Err(error.to_string());
        }
    }

    // The persisted folder is the source of truth, even when its watcher is deferred.
    set_sync_enabled(app, true);

    if registration == watcher::WatchRegistration::PathUnavailable {
        log_to_ui_folder(
            app,
            "WARN",
            &format!(
                "Folder '{}' does not exist; watching deferred until it appears.",
                path_string
            ),
            "log_folder_deferred",
            &path_string,
        );
    }

    // An offline folder will be scanned when the deferred-watch task registers it.
    if is_available {
        let pool_inner = pool.clone();
        let path_clone = path_string.clone();
        let app_for_scan = app.clone();
        tauri::async_runtime::spawn(async move {
            let scan_result =
                match scan_folders_for_media(vec![PathBuf::from(path_clone.clone())]).await {
                    Ok(result) => result,
                    Err(error) => {
                        log_to_ui_folder_error(
                            &app_for_scan,
                            "ERROR",
                            &format!("Folder scanning failed for '{}': {}", path_clone, error),
                            "log_folder_scan_failed",
                            &path_clone,
                            &error,
                        );
                        return;
                    }
                };
            sync_scan_result_if_authenticated(app_for_scan, pool_inner, scan_result, "folder_add")
                .await;
        });
    }

    audit_event(
        app,
        crate::audit::AuditEvent::new(
            crate::audit::AuditEvent::operation_id(),
            "folder.added",
            crate::audit::Outcome::Success,
            crate::audit::Severity::Info,
            None,
            "command",
            "Watched folder added",
            json!({ "folder_id": id, "local_path": path_string, "target_album_id": target_album_id }),
        ),
    );
    Ok(id)
}

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
pub(crate) async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    path: String,
    target_album_id: Option<String>,
) -> Result<i64, String> {
    use tauri::Manager;
    let mut params = RegisterFolderParams::new(&path, target_album_id);
    if let Some(active_volumes) = app.try_state::<crate::removable::ActiveRemovableVolumes>() {
        if let Some(binding) = detect_removable_binding_for_path(&active_volumes, Path::new(&path))
        {
            params.removable_binding = Some(binding);
        }
    }

    register_watched_folder(&app, pool.inner(), watcher.inner(), params).await
}

pub(crate) fn detect_removable_binding_for_path(
    active_volumes: &crate::removable::ActiveRemovableVolumes,
    path: &Path,
) -> Option<RemovableBindingInput> {
    let normalized_path = normalize_folder_path(path);
    let volumes = active_volumes.0.lock().ok()?;
    let mut matching_volumes: Vec<&crate::removable::VolumeInfo> = volumes
        .values()
        .filter(|vol| {
            let normalized_mount = normalize_folder_path(Path::new(&vol.mount_path));
            path_is_within(&normalized_path, &normalized_mount)
        })
        .collect();

    matching_volumes.sort_by_key(|vol| std::cmp::Reverse(vol.mount_path.len()));

    if let Some(best_volume) = matching_volumes.first() {
        let normalized_mount = normalize_folder_path(Path::new(&best_volume.mount_path));
        let relative_path =
            compute_volume_relative_path(&normalized_mount, &normalized_path).ok()?;
        Some(RemovableBindingInput {
            volume_id: best_volume.volume_id.clone(),
            relative_path,
        })
    } else {
        None
    }
}

pub(crate) fn compute_volume_relative_path(
    mount_path: &Path,
    folder_path: &Path,
) -> Result<String, String> {
    let normalized_mount = normalize_folder_path(mount_path);
    let normalized_folder = normalize_folder_path(folder_path);

    if !path_is_within(&normalized_folder, &normalized_mount) {
        return Err(format!(
            "Path '{}' is not within mount path '{}'",
            folder_path.display(),
            mount_path.display()
        ));
    }

    let mount_count = normalized_mount.components().count();
    let folder_components: Vec<_> = normalized_folder.components().collect();

    if folder_components.len() <= mount_count {
        return Ok(String::new());
    }

    let remaining = &folder_components[mount_count..];
    let mut parts = Vec::new();
    for component in remaining {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }
    Ok(parts.join("/"))
}

pub(crate) fn validate_removable_folder_acceptance(
    volume: Option<&crate::removable::VolumeInfo>,
    volume_id: &str,
    path: &Path,
    existing_bindings: &[crate::db::RemovableFolderBinding],
) -> Result<String, String> {
    let volume =
        volume.ok_or_else(|| format!("Removable volume '{}' is not connected.", volume_id))?;

    if !existing_bindings.is_empty() {
        return Err(format!(
            "Removable volume '{}' is already bound to a watched folder.",
            volume_id
        ));
    }

    let mount_path = Path::new(&volume.mount_path);
    let normalized_mount = normalize_folder_path(mount_path);
    let normalized_path = normalize_folder_path(path);

    if !path_is_within(&normalized_path, &normalized_mount) {
        return Err(format!(
            "Path '{}' does not reside within removable volume '{}' (mounted at '{}').",
            path.display(),
            volume_id,
            volume.mount_path
        ));
    }

    compute_volume_relative_path(&normalized_mount, &normalized_path)
}

#[tauri::command]
pub(crate) async fn accept_removable_media_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    active_volumes: tauri::State<'_, crate::removable::ActiveRemovableVolumes>,
    volume_id: String,
    path: String,
    target_album_id: Option<String>,
) -> Result<i64, String> {
    let volume = active_volumes.get(&volume_id);
    let existing_bindings = db::get_removable_folders_by_volume(pool.inner(), &volume_id)
        .await
        .map_err(|error| error.to_string())?;

    let relative_path = validate_removable_folder_acceptance(
        volume.as_ref(),
        &volume_id,
        Path::new(&path),
        &existing_bindings,
    )?;

    let params = RegisterFolderParams::new(&path, target_album_id)
        .with_removable_binding(volume_id, relative_path);

    register_watched_folder(&app, pool.inner(), watcher.inner(), params).await
}

#[tauri::command]
pub(crate) async fn get_folders(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::WatchedFolder>, String> {
    db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn update_folder_exclusion_patterns(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    id: i64,
    exclusion_patterns: Vec<String>,
) -> Result<(), String> {
    let exclusion_patterns = normalize_exclusion_patterns(exclusion_patterns)?;
    if !db::update_exclusion_patterns(pool.inner(), id, &exclusion_patterns)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("Watched folder was not found.".to_string());
    }
    let folder = db::get_folders(pool.inner())
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|folder| folder.id == id)
        .ok_or("Watched folder was not found.")?;
    let scan_result = scan_watched_folders_for_media(vec![folder]).await?;
    sync_scan_result_if_authenticated(
        app,
        pool.inner().clone(),
        scan_result,
        "exclusion_patterns_changed",
    )
    .await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
    id: i64,
    confirm_cancellation: Option<bool>,
) -> Result<RemoveFolderResult, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let path = folder.path.clone();
        let has_active_work = sync_coordinator.0.has_active_work_in(&path).await
            || db::has_sync_work_in_folder(pool.inner(), &path)
                .await
                .map_err(|error| error.to_string())?;
        if removal_requires_confirmation(confirm_cancellation) {
            return Ok(RemoveFolderResult {
                requires_sync_cancellation: has_active_work,
            });
        }
        {
            let mut w = watcher.lock().await;
            if let Err(e) = watcher::unwatch_path(&mut w, &path) {
                log_to_ui_folder_error(
                    &app,
                    "WARN",
                    &format!("Failed to unwatch '{}': {}", path, e),
                    "log_unwatch_failed",
                    &path,
                    &e.to_string(),
                );
                return Err(format!("Failed to unwatch '{}': {}", path, e));
            }
        }

        // The watcher is gone before cancellation so no new work can be produced for this root.
        // The queue cleanup is path-component based and leaves sibling roots untouched.
        sync_coordinator.0.cancel_work_in(&path).await;
        if let Err(error) = db::discard_sync_data_in_folder(pool.inner(), &path).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            let _ = w.reconcile_path(&path, is_available);
            return Err(error.to_string());
        }

        if let Err(error) = db::remove_folder(pool.inner(), id).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            if let Err(rewatch_error) = w.reconcile_path(&path, is_available) {
                log_to_ui_folder_error(
                    &app,
                    "ERROR",
                    &format!(
                        "Could not restore watcher registration for '{}': {}",
                        path, rewatch_error
                    ),
                    "log_watcher_restore_failed",
                    &path,
                    &rewatch_error.to_string(),
                );
            }
            return Err(error.to_string());
        }
    } else {
        db::remove_folder(pool.inner(), id)
            .await
            .map_err(|e| e.to_string())?;
    }
    match db::get_folders(pool.inner()).await {
        Ok(folders) => set_sync_enabled(&app, !folders.is_empty()),
        Err(error) => log_to_ui_error(
            &app,
            "ERROR",
            &format!(
                "Folder was removed, but could not refresh tray sync availability: {}",
                error
            ),
            "log_tray_refresh_after_remove_failed",
            &error.to_string(),
        ),
    }
    audit_event(
        &app,
        crate::audit::AuditEvent::new(
            crate::audit::AuditEvent::operation_id(),
            "folder.removed",
            crate::audit::Outcome::Success,
            crate::audit::Severity::Info,
            None,
            "command",
            "Watched folder removed",
            json!({ "folder_id": id }),
        ),
    );
    Ok(RemoveFolderResult {
        requires_sync_cancellation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{removal_requires_confirmation, RemoveFolderResult};

    #[test]
    fn folder_removal_requires_explicit_confirmation() {
        assert!(removal_requires_confirmation(None));
        assert!(removal_requires_confirmation(Some(false)));
        assert!(!removal_requires_confirmation(Some(true)));

        let result = RemoveFolderResult {
            requires_sync_cancellation: true,
        };
        assert!(result.requires_sync_cancellation);
    }

    #[test]
    fn folder_conflict_check_detects_duplicates_and_overlaps() {
        use std::path::{Path, PathBuf};
        let existing = vec![PathBuf::from("C:/Media/Photos")];

        // Exact duplicate
        let err =
            super::check_folder_conflict(Path::new("C:/Media/Photos"), &existing).unwrap_err();
        assert!(err.contains("already being watched"));

        // Child subfolder
        let err =
            super::check_folder_conflict(Path::new("C:/Media/Photos/2024"), &existing).unwrap_err();
        assert!(err.contains("overlaps with already watched folder"));

        // Parent folder
        let err = super::check_folder_conflict(Path::new("C:/Media"), &existing).unwrap_err();
        assert!(err.contains("overlaps with already watched folder"));

        // Sibling folder with common prefix
        let ok =
            super::check_folder_conflict(Path::new("C:/Media/Photos-Vacation"), &existing).unwrap();
        assert_eq!(
            ok,
            crate::media::paths::normalize_folder_path(Path::new("C:/Media/Photos-Vacation"))
        );
    }

    #[test]
    fn target_album_sanitization_trims_and_discards_empty_strings() {
        assert_eq!(super::sanitize_target_album_id(None), None);
        assert_eq!(
            super::sanitize_target_album_id(Some("   ".to_string())),
            None
        );
        assert_eq!(
            super::sanitize_target_album_id(Some("  album-uuid  ".to_string())),
            Some("album-uuid".to_string())
        );
    }

    #[test]
    fn exclusion_patterns_are_normalized_deduplicated_and_validated() {
        assert_eq!(
            super::normalize_exclusion_patterns(vec![
                " Screenshots/** ".into(),
                "screenshots/**".into(),
                "".into()
            ])
            .unwrap(),
            vec!["Screenshots/**"]
        );
        assert!(super::normalize_exclusion_patterns(vec!["../private/**".into()]).is_err());
        for invalid in ["Screenshots\\**", "/private/**", "!keep.jpg", "*.{jpg,png}"] {
            assert!(super::normalize_exclusion_patterns(vec![invalid.into()]).is_err());
        }
    }

    #[test]
    fn register_folder_params_constructs_manual_and_removable_bindings() {
        let manual = super::RegisterFolderParams::new("C:/Photos", Some("album-1".to_string()));
        assert_eq!(manual.path, "C:/Photos");
        assert_eq!(manual.target_album_id.as_deref(), Some("album-1"));
        assert!(manual.removable_binding.is_none());

        let removable = super::RegisterFolderParams::new("E:/DCIM", None)
            .with_removable_binding("vol-uuid-456", "DCIM");
        assert_eq!(removable.path, "E:/DCIM");
        assert_eq!(
            removable.removable_binding,
            Some(super::RemovableBindingInput {
                volume_id: "vol-uuid-456".to_string(),
                relative_path: "DCIM".to_string(),
            })
        );
    }

    #[test]
    fn validate_removable_acceptance_checks_presence_bounds_and_containment() {
        use crate::removable::VolumeInfo;
        use std::path::Path;

        let volume = VolumeInfo {
            volume_id: "vol-sd-123".to_string(),
            mount_path: if cfg!(windows) {
                "E:\\".to_string()
            } else {
                "/media/sdcard".to_string()
            },
            display_name: "SD_CARD".to_string(),
        };

        // 1. Disconnected volume
        let err = super::validate_removable_folder_acceptance(
            None,
            "vol-missing",
            Path::new(if cfg!(windows) {
                "E:\\DCIM"
            } else {
                "/media/sdcard/DCIM"
            }),
            &[],
        )
        .unwrap_err();
        assert!(err.contains("not connected") || err.contains("not found"));

        // 2. Already bound volume
        let existing_binding = crate::db::RemovableFolderBinding {
            watched_folder_id: 1,
            volume_id: "vol-sd-123".to_string(),
            relative_path: "DCIM".to_string(),
        };
        let err = super::validate_removable_folder_acceptance(
            Some(&volume),
            "vol-sd-123",
            Path::new(if cfg!(windows) {
                "E:\\DCIM"
            } else {
                "/media/sdcard/DCIM"
            }),
            &[existing_binding],
        )
        .unwrap_err();
        assert!(err.contains("already bound"));

        // 3. Path outside volume mount
        let outside_path = Path::new(if cfg!(windows) {
            "C:\\Photos"
        } else {
            "/home/user/Photos"
        });
        let err = super::validate_removable_folder_acceptance(
            Some(&volume),
            "vol-sd-123",
            outside_path,
            &[],
        )
        .unwrap_err();
        assert!(err.contains("does not reside within") || err.contains("outside"));

        // 4. Valid path inside volume
        let valid_path = Path::new(if cfg!(windows) {
            "E:\\DCIM\\100EOS"
        } else {
            "/media/sdcard/DCIM/100EOS"
        });
        let relative = super::validate_removable_folder_acceptance(
            Some(&volume),
            "vol-sd-123",
            valid_path,
            &[],
        )
        .unwrap();
        assert_eq!(relative.replace('\\', "/"), "DCIM/100EOS");

        // 5. Valid path at mount root
        let root_path = Path::new(&volume.mount_path);
        let relative_root = super::validate_removable_folder_acceptance(
            Some(&volume),
            "vol-sd-123",
            root_path,
            &[],
        )
        .unwrap();
        assert_eq!(relative_root, "");
    }

    #[test]
    fn detect_removable_binding_for_path_resolves_mounted_volumes() {
        use crate::removable::{ActiveRemovableVolumes, VolumeInfo};
        use std::path::Path;

        let active_volumes = ActiveRemovableVolumes::new();
        let volume = VolumeInfo {
            volume_id: "vol-card-456".to_string(),
            mount_path: if cfg!(windows) {
                "E:\\".to_string()
            } else {
                "/media/sdcard".to_string()
            },
            display_name: "SD_CARD".to_string(),
        };
        active_volumes.insert(volume);

        // Path on removable card
        let card_path = Path::new(if cfg!(windows) {
            "E:\\DCIM\\100CANON"
        } else {
            "/media/sdcard/DCIM/100CANON"
        });
        let binding = super::detect_removable_binding_for_path(&active_volumes, card_path);
        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert_eq!(binding.volume_id, "vol-card-456");
        assert_eq!(binding.relative_path.replace('\\', "/"), "DCIM/100CANON");

        // Path on fixed local drive
        let fixed_path = Path::new(if cfg!(windows) {
            "C:\\Users\\Photos"
        } else {
            "/home/user/Photos"
        });
        let no_binding = super::detect_removable_binding_for_path(&active_volumes, fixed_path);
        assert!(no_binding.is_none());
    }
}
