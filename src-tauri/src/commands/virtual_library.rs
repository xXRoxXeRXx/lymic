//! Thin Tauri adapters for the Virtual Library settings section (ADR-0003).
//!
//! The section exists only for a packaged Store-channel Windows run. The decision is made here, at
//! runtime, and never by the frontend: the release-channel flag alone is a compile-time label.
//! The toggle has no projection effect yet; later settings (root folder, overlap refusals) extend
//! `VirtualLibrarySettings` and `change_enabled`.

use crate::{app::package_identity, commands::settings::effective_release_channel, db};
use serde::Serialize;
use tauri::Emitter;

pub(crate) const SETTINGS_CHANGED_EVENT: &str = "virtual-library-settings-changed";
const UNAVAILABLE_MESSAGE: &str =
    "The Virtual Library is available only in the Microsoft Store version of Lymic on Windows.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VirtualLibrarySettings {
    pub(crate) available: bool,
    pub(crate) enabled: bool,
}

/// The availability rule: Windows, the Store release channel and a real Package Identity together.
pub(crate) fn is_available(
    is_windows: bool,
    release_channel: &str,
    has_package_identity: bool,
) -> bool {
    is_windows && release_channel == "store" && has_package_identity
}

fn detect_availability() -> bool {
    is_available(
        cfg!(windows),
        effective_release_channel(),
        package_identity::has_package_identity(),
    )
}

async fn load_settings(
    pool: &sqlx::SqlitePool,
    available: bool,
) -> Result<VirtualLibrarySettings, String> {
    let enabled = available
        && db::get_virtual_library_enabled(pool)
            .await
            .map_err(|error| error.to_string())?;
    Ok(VirtualLibrarySettings { available, enabled })
}

async fn change_enabled(
    pool: &sqlx::SqlitePool,
    available: bool,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Err(UNAVAILABLE_MESSAGE.to_string());
    }
    db::set_virtual_library_enabled(pool, enabled)
        .await
        .map_err(|error| error.to_string())?;
    load_settings(pool, available).await
}

#[tauri::command]
pub(crate) async fn get_virtual_library_settings(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<VirtualLibrarySettings, String> {
    load_settings(pool.inner(), detect_availability()).await
}

#[tauri::command]
pub(crate) async fn set_virtual_library_enabled(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    let settings = change_enabled(pool.inner(), detect_availability(), enabled).await?;
    // The change is already persisted; a failed announcement only delays other views.
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &settings);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn available_only_on_windows_in_the_store_channel_with_package_identity() {
        assert!(is_available(true, "store", true));
        for (is_windows, channel, identity) in [
            (false, "store", true),
            (true, "github", true),
            (true, "store", false),
            (false, "github", false),
            (false, "store", false),
            (true, "github", false),
        ] {
            assert!(
                !is_available(is_windows, channel, identity),
                "windows={is_windows} channel={channel} identity={identity}"
            );
        }
    }

    #[test]
    fn a_store_labelled_unpackaged_run_is_unavailable() {
        // The release-channel flag says store, but the test binary has no Package Identity.
        assert!(!is_available(
            cfg!(windows),
            "store",
            package_identity::has_package_identity()
        ));
    }

    #[tokio::test]
    async fn settings_start_disabled_and_the_enabled_choice_persists_when_available() {
        let pool = test_pool().await;

        assert_eq!(
            load_settings(&pool, true).await.unwrap(),
            VirtualLibrarySettings {
                available: true,
                enabled: false
            }
        );
        assert_eq!(
            change_enabled(&pool, true, true).await.unwrap(),
            VirtualLibrarySettings {
                available: true,
                enabled: true
            }
        );
        assert!(load_settings(&pool, true).await.unwrap().enabled);
        assert!(!change_enabled(&pool, true, false).await.unwrap().enabled);
    }

    #[tokio::test]
    async fn unavailable_settings_are_never_enabled_and_cannot_be_changed() {
        let pool = test_pool().await;
        db::set_virtual_library_enabled(&pool, true).await.unwrap();

        assert_eq!(
            load_settings(&pool, false).await.unwrap(),
            VirtualLibrarySettings {
                available: false,
                enabled: false
            }
        );
        assert_eq!(
            change_enabled(&pool, false, true).await,
            Err(UNAVAILABLE_MESSAGE.to_string())
        );
    }

    #[test]
    fn settings_serialise_with_the_field_names_the_frontend_contract_expects() {
        assert_eq!(
            serde_json::to_value(VirtualLibrarySettings {
                available: true,
                enabled: false
            })
            .unwrap(),
            serde_json::json!({ "available": true, "enabled": false })
        );
    }
}
