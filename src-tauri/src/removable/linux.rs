#[cfg(target_os = "linux")]
use super::model::VolumeEvent;
#[cfg(target_os = "linux")]
use super::monitor::RemovableMediaMonitor;
#[cfg(target_os = "linux")]
use tokio::sync::mpsc::Sender;

#[cfg(target_os = "linux")]
pub struct LinuxRemovableMediaMonitor;

#[cfg(target_os = "linux")]
impl Default for LinuxRemovableMediaMonitor {
    fn default() -> Self {
        Self
    }
}

#[cfg(target_os = "linux")]
impl RemovableMediaMonitor for LinuxRemovableMediaMonitor {
    fn start(&self, _tx: Sender<VolumeEvent>) -> Result<(), String> {
        // Linux udev monitor hook
        Ok(())
    }
}
