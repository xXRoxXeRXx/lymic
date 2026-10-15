use super::model::VolumeEvent;
use tokio::sync::mpsc::Sender;

pub trait RemovableMediaMonitor: Send + Sync {
    fn start(&self, tx: Sender<VolumeEvent>) -> Result<(), String>;
}

#[allow(dead_code)]
#[derive(Default)]
pub struct MockRemovableMediaMonitor;

impl RemovableMediaMonitor for MockRemovableMediaMonitor {
    fn start(&self, _tx: Sender<VolumeEvent>) -> Result<(), String> {
        Ok(())
    }
}
