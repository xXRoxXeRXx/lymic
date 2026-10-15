pub(crate) mod coordinator;
pub(crate) mod model;
pub(crate) mod monitor;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(test)]
mod tests;

pub(crate) use coordinator::*;
pub(crate) use model::*;
pub(crate) use monitor::*;

use tauri::{Emitter, Manager};

impl VolumeEventEmitter for tauri::AppHandle {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String> {
        self.emit("removable-volume-detected", volume)
            .map_err(|e| e.to_string())
    }

    fn emit_volume_removed(&self, volume_id: &str) -> Result<(), String> {
        self.emit("removable-volume-removed", volume_id)
            .map_err(|e| e.to_string())
    }

    async fn reconcile_remount_watcher(
        &self,
        old_path: Option<&str>,
        new_path: &str,
    ) -> Result<(), String> {
        let watcher_state = self.state::<tokio::sync::Mutex<crate::watcher::WatcherState>>();
        let mut w = watcher_state.lock().await;

        if let Some(old) = old_path {
            let _ = crate::watcher::unwatch_path(&mut w, old);
        }

        w.reconcile_path(new_path, true)
            .map_err(|e| format!("Could not watch '{new_path}': {e}"))?;

        Ok(())
    }

    async fn reconcile_folder_unavailable(&self, path: &str) -> Result<(), String> {
        let watcher_state = self.state::<tokio::sync::Mutex<crate::watcher::WatcherState>>();
        let mut w = watcher_state.lock().await;
        w.reconcile_path(path, false)
            .map_err(|e| format!("Could not unwatch unavailable path '{path}': {e}"))?;
        Ok(())
    }

    async fn trigger_remount_scan(&self, folder_path: &str) -> Result<(), String> {
        let path = std::path::PathBuf::from(folder_path);
        let app = self.clone();
        let pool = self.state::<sqlx::SqlitePool>().inner().clone();
        let path_str = folder_path.to_string();

        tauri::async_runtime::spawn(async move {
            let scan_result = match crate::sync::scan_folders_for_media(vec![path]).await {
                Ok(result) => result,
                Err(error) => {
                    crate::app::events::log_to_ui_folder_error(
                        &app,
                        "ERROR",
                        &format!("Folder scanning failed for '{path_str}': {error}"),
                        "log_folder_scan_failed",
                        &path_str,
                        &error,
                    );
                    return;
                }
            };
            crate::sync::sync_scan_result_if_authenticated(
                app,
                pool,
                scan_result,
                "removable_media_reconnect",
            )
            .await;
        });

        Ok(())
    }
}

pub fn create_platform_monitor() -> Box<dyn RemovableMediaMonitor> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsRemovableMediaMonitor::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsRemovableMediaMonitor)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxRemovableMediaMonitor)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Box::new(monitor::MockRemovableMediaMonitor)
    }
}
