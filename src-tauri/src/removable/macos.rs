#[cfg(target_os = "macos")]
use super::model::VolumeEvent;
#[cfg(target_os = "macos")]
use super::monitor::RemovableMediaMonitor;
#[cfg(target_os = "macos")]
use tokio::sync::mpsc::Sender;

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct MacOsRemovableMediaMonitor;

#[cfg(target_os = "macos")]
impl RemovableMediaMonitor for MacOsRemovableMediaMonitor {
    fn start(&self, _tx: Sender<VolumeEvent>) -> Result<(), String> {
        // macOS Disk Arbitration framework monitor hook
        Ok(())
    }
}
