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

use tauri::Emitter;

impl VolumeEventEmitter for tauri::AppHandle {
    fn emit_volume_detected(&self, volume: &VolumeInfo) -> Result<(), String> {
        self.emit("removable-volume-detected", volume)
            .map_err(|e| e.to_string())
    }
}

pub fn create_platform_monitor() -> Box<dyn RemovableMediaMonitor> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsRemovableMediaMonitor::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsRemovableMediaMonitor::default())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxRemovableMediaMonitor::default())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Box::new(monitor::MockRemovableMediaMonitor)
    }
}
