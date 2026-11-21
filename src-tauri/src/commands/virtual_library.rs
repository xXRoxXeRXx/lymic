//! Thin Tauri adapters for the Virtual Library settings section (ADR-0003).
//!
//! The section exists only for a packaged Store-channel Windows run. The decision is made here, at
//! runtime, and never by the frontend: the release-channel flag alone is a compile-time label.
//! The toggle has no projection effect yet. The root folder (the Known Folder Pictures plus
//! `Lymic` unless the user chose another) and the refusal to overlap a Watched Folder live here
//! and in `virtual_library_root`; the registration side of the rule is in `watched_folders`.

use crate::{
    db,
    media::paths::canonicalize_folder_path,
    virtual_library_root::{
        self, is_available_at_runtime, load_root, KnownFolders, RootOverlap, SystemKnownFolders,
    },
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Emitter;

pub(crate) const SETTINGS_CHANGED_EVENT: &str = "virtual-library-settings-changed";
const UNAVAILABLE_MESSAGE: &str =
    "The Virtual Library is available only in the Microsoft Store version of Lymic on Windows.";
const NO_ROOT_MESSAGE: &str = "The Virtual Library cannot be enabled because Lymic could not find your Pictures folder. Choose a Virtual Library folder first.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VirtualLibrarySettings {
    pub(crate) available: bool,
    pub(crate) enabled: bool,
    /// The effective Virtual Library folder: the user's choice, or the default. `None` while the
    /// Virtual Library is unavailable or the system cannot say where Pictures is.
    pub(crate) root_folder: Option<String>,
}

async fn load_settings(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Ok(VirtualLibrarySettings {
            available,
            enabled: false,
            root_folder: None,
        });
    }
    let enabled = db::get_virtual_library_enabled(pool)
        .await
        .map_err(|error| error.to_string())?;
    let root_folder = load_root(pool, known_folders)
        .await?
        .map(|root| root.to_string_lossy().into_owned());
    Ok(VirtualLibrarySettings {
        available,
        enabled,
        root_folder,
    })
}

/// The Watched Folders that overlap `root`, and how.
async fn overlapping_watched_folders(
    pool: &sqlx::SqlitePool,
    root: &Path,
) -> Result<Vec<(PathBuf, RootOverlap)>, String> {
    let folders = db::get_folders(pool)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|folder| PathBuf::from(folder.path))
        .collect::<Vec<_>>();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || virtual_library_root::overlapping_folders(&root, &folders))
        .await
        .map_err(|error| error.to_string())
}

/// Names every overlapping Watched Folder so the user knows what to remove.
fn overlap_refusal(action: &str, root: &Path, overlaps: &[(PathBuf, RootOverlap)]) -> String {
    let remedy =
        "Remove it from your Watched Folders or choose a different Virtual Library folder.";
    match overlaps {
        [(folder, relation)] => format!(
            "{action}: the Watched Folder '{}' {} '{}'. {remedy}",
            folder.display(),
            relation.describe(),
            root.display()
        ),
        _ => {
            let names = overlaps
                .iter()
                .map(|(folder, _)| format!("'{}'", folder.display()))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{action}: the Watched Folders {names} overlap the Virtual Library folder '{}'. {remedy}",
                root.display()
            )
        }
    }
}

async fn change_enabled(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Err(UNAVAILABLE_MESSAGE.to_string());
    }
    if enabled {
        let root = load_root(pool, known_folders)
            .await?
            .ok_or_else(|| NO_ROOT_MESSAGE.to_string())?;
        let overlaps = overlapping_watched_folders(pool, &root).await?;
        if !overlaps.is_empty() {
            return Err(overlap_refusal(
                "The Virtual Library cannot be enabled",
                &root,
                &overlaps,
            ));
        }
    }
    db::set_virtual_library_enabled(pool, enabled)
        .await
        .map_err(|error| error.to_string())?;
    load_settings(pool, available, known_folders).await
}

async fn change_root_folder(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
    chosen: &str,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Err(UNAVAILABLE_MESSAGE.to_string());
    }
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return Err("Choose a folder for the Virtual Library.".to_string());
    }
    if !Path::new(chosen).is_absolute() {
        return Err(format!(
            "'{chosen}' is not a full folder path. Choose the Virtual Library folder with the folder picker."
        ));
    }
    let chosen_path = PathBuf::from(chosen);
    let root = tokio::task::spawn_blocking(move || {
        let root = canonicalize_folder_path(&chosen_path);
        if root.exists() && !root.is_dir() {
            return Err(format!("'{}' is not a folder.", root.display()));
        }
        Ok(root)
    })
    .await
    .map_err(|error| error.to_string())??;

    let overlaps = overlapping_watched_folders(pool, &root).await?;
    if !overlaps.is_empty() {
        return Err(overlap_refusal(
            &format!("'{}' cannot be the Virtual Library folder", root.display()),
            &root,
            &overlaps,
        ));
    }
    db::set_virtual_library_root(pool, &root.to_string_lossy())
        .await
        .map_err(|error| error.to_string())?;
    load_settings(pool, available, known_folders).await
}

fn announce(app: &tauri::AppHandle, settings: &VirtualLibrarySettings) {
    // The change is already persisted; a failed announcement only delays other views.
    let _ = app.emit(SETTINGS_CHANGED_EVENT, settings);
}

#[tauri::command]
pub(crate) async fn get_virtual_library_settings(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<VirtualLibrarySettings, String> {
    load_settings(pool.inner(), is_available_at_runtime(), &SystemKnownFolders).await
}

#[tauri::command]
pub(crate) async fn set_virtual_library_enabled(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    let settings = change_enabled(
        pool.inner(),
        is_available_at_runtime(),
        &SystemKnownFolders,
        enabled,
    )
    .await?;
    announce(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn set_virtual_library_root_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    path: String,
) -> Result<VirtualLibrarySettings, String> {
    let settings = change_root_folder(
        pool.inner(),
        is_available_at_runtime(),
        &SystemKnownFolders,
        &path,
    )
    .await?;
    announce(&app, &settings);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::virtual_library_root::FixedKnownFolders;

    fn base() -> PathBuf {
        std::env::temp_dir().join("lymic-vl-settings-tests")
    }

    fn pictures() -> PathBuf {
        base().join("Pictures")
    }

    fn default_root() -> PathBuf {
        pictures().join("Lymic")
    }

    fn known() -> FixedKnownFolders {
        FixedKnownFolders(Some(pictures()))
    }

    fn settings(available: bool, enabled: bool, root: Option<PathBuf>) -> VirtualLibrarySettings {
        VirtualLibrarySettings {
            available,
            enabled,
            root_folder: root.map(|root| root.to_string_lossy().into_owned()),
        }
    }

    async fn watch(pool: &sqlx::SqlitePool, path: &Path) {
        db::add_folder(pool, &path.to_string_lossy(), None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn settings_start_disabled_and_the_enabled_choice_persists_when_available() {
        let pool = test_pool().await;
        let known = known();

        assert_eq!(
            load_settings(&pool, true, &known).await.unwrap(),
            settings(true, false, Some(default_root()))
        );
        assert_eq!(
            change_enabled(&pool, true, &known, true).await.unwrap(),
            settings(true, true, Some(default_root()))
        );
        assert!(load_settings(&pool, true, &known).await.unwrap().enabled);
        assert!(
            !change_enabled(&pool, true, &known, false)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn unavailable_settings_are_never_enabled_and_cannot_be_changed() {
        let pool = test_pool().await;
        let known = known();
        db::set_virtual_library_enabled(&pool, true).await.unwrap();

        assert_eq!(
            load_settings(&pool, false, &known).await.unwrap(),
            settings(false, false, None)
        );
        assert_eq!(
            change_enabled(&pool, false, &known, true).await,
            Err(UNAVAILABLE_MESSAGE.to_string())
        );
        assert_eq!(
            change_root_folder(&pool, false, &known, &default_root().to_string_lossy()).await,
            Err(UNAVAILABLE_MESSAGE.to_string())
        );
    }

    #[test]
    fn settings_serialise_with_the_field_names_the_frontend_contract_expects() {
        assert_eq!(
            serde_json::to_value(VirtualLibrarySettings {
                available: true,
                enabled: false,
                root_folder: Some("C:\\Pictures\\Lymic".to_string()),
            })
            .unwrap(),
            serde_json::json!({
                "available": true,
                "enabled": false,
                "root_folder": "C:\\Pictures\\Lymic"
            })
        );
    }

    #[tokio::test]
    async fn the_root_defaults_to_lymic_in_the_known_folder_pictures_and_can_be_changed() {
        let pool = test_pool().await;
        let known = known();
        let chosen = base().join("Elsewhere");

        let changed = change_root_folder(&pool, true, &known, &chosen.to_string_lossy())
            .await
            .unwrap();

        assert_eq!(
            changed,
            settings(true, false, Some(canonicalize_folder_path(&chosen)))
        );
        assert_eq!(load_settings(&pool, true, &known).await.unwrap(), changed);
    }

    #[tokio::test]
    async fn there_is_no_root_until_one_is_chosen_when_pictures_cannot_be_resolved() {
        let pool = test_pool().await;
        let unknown = FixedKnownFolders(None);

        assert_eq!(
            load_settings(&pool, true, &unknown).await.unwrap(),
            settings(true, false, None)
        );
        assert_eq!(
            change_enabled(&pool, true, &unknown, true).await,
            Err(NO_ROOT_MESSAGE.to_string())
        );
        assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn an_invalid_root_choice_is_refused_and_leaves_the_root_unchanged() {
        let pool = test_pool().await;
        let known = known();

        assert!(change_root_folder(&pool, true, &known, "   ")
            .await
            .is_err());
        assert!(change_root_folder(&pool, true, &known, "relative/folder")
            .await
            .is_err());
        let file = std::env::temp_dir().join(format!("lymic-vl-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"x").unwrap();
        let error = change_root_folder(&pool, true, &known, &file.to_string_lossy())
            .await
            .unwrap_err();
        std::fs::remove_file(&file).unwrap();

        assert!(error.contains("is not a folder"), "{error}");
        assert_eq!(
            load_settings(&pool, true, &known)
                .await
                .unwrap()
                .root_folder,
            Some(default_root().to_string_lossy().into_owned())
        );
    }

    #[tokio::test]
    async fn enabling_is_refused_while_a_watched_folder_overlaps_the_root_and_names_it() {
        let known = known();
        for (watched, relation) in [
            (pictures(), "contains the Virtual Library folder"),
            (default_root(), "is the Virtual Library folder"),
            (
                default_root().join("Albums"),
                "is inside the Virtual Library folder",
            ),
        ] {
            let pool = test_pool().await;
            watch(&pool, &watched).await;

            let error = change_enabled(&pool, true, &known, true).await.unwrap_err();

            assert!(
                error.contains(&format!("'{}'", watched.display())),
                "names the folder: {error}"
            );
            assert!(error.contains(relation), "{error}");
            assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
        }
    }

    #[tokio::test]
    async fn an_unrelated_watched_folder_does_not_block_enabling_and_disabling_is_never_refused() {
        let pool = test_pool().await;
        let known = known();
        watch(&pool, &pictures().join("Holiday")).await;

        assert!(
            change_enabled(&pool, true, &known, true)
                .await
                .unwrap()
                .enabled
        );
        watch(&pool, &pictures()).await;
        assert!(
            !change_enabled(&pool, true, &known, false)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn enabling_names_every_overlapping_watched_folder() {
        let pool = test_pool().await;
        let inside = default_root().join("Albums");
        watch(&pool, &pictures()).await;
        watch(&pool, &inside).await;

        let error = change_enabled(&pool, true, &known(), true)
            .await
            .unwrap_err();

        assert!(
            error.contains(&format!("'{}'", pictures().display())),
            "{error}"
        );
        assert!(
            error.contains(&format!("'{}'", inside.display())),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_root_that_overlaps_a_watched_folder_is_refused_and_names_it() {
        let pool = test_pool().await;
        let known = known();
        let watched = base().join("Camera");
        watch(&pool, &watched).await;

        let error = change_root_folder(
            &pool,
            true,
            &known,
            &watched.join("Lymic").to_string_lossy(),
        )
        .await
        .unwrap_err();

        assert!(
            error.contains(&format!("'{}'", watched.display())),
            "{error}"
        );
        assert_eq!(
            load_settings(&pool, true, &known)
                .await
                .unwrap()
                .root_folder,
            Some(default_root().to_string_lossy().into_owned())
        );
    }
}
