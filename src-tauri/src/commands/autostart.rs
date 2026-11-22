//! Thin Tauri adapters for the autostart toggle in the settings.
//!
//! The mechanism is chosen here, at runtime: the Windows StartupTask when the process has Package
//! Identity (a registry Run entry written from inside an MSIX package is virtualized, see
//! `app::autostart`), the `tauri-plugin-autostart` login item otherwise.

use crate::app::{autostart, package_identity::has_package_identity};
use tauri_plugin_autostart::ManagerExt;

async fn on_blocking_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn get_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    if has_package_identity() {
        on_blocking_thread(autostart::packaged_autostart_enabled).await
    } else {
        app.autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub(crate) async fn set_autostart_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    if has_package_identity() {
        on_blocking_thread(move || autostart::set_packaged_autostart(enabled)).await
    } else if enabled {
        app.autolaunch().enable().map_err(|error| error.to_string())
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())
    }
}
