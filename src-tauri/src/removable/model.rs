#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub volume_id: String,
    pub mount_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VolumeRemoval {
    MountPath(String),
    VolumeId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeEvent {
    Arrival(VolumeInfo),
    Removal(VolumeRemoval),
}

#[derive(Clone, Default)]
pub struct ActiveRemovableVolumes(
    pub std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, VolumeInfo>>>,
);

impl ActiveRemovableVolumes {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )))
    }

    pub fn insert(&self, volume: VolumeInfo) {
        if let Ok(mut lock) = self.0.lock() {
            lock.insert(volume.volume_id.clone(), volume);
        }
    }

    pub fn remove(&self, volume_id: &str) {
        if let Ok(mut lock) = self.0.lock() {
            lock.remove(volume_id);
        }
    }

    pub fn remove_by_mount_path(&self, mount_path: &str) {
        if let Ok(mut lock) = self.0.lock() {
            lock.retain(|_, v| v.mount_path != mount_path);
        }
    }

    pub fn get(&self, volume_id: &str) -> Option<VolumeInfo> {
        self.0.lock().ok()?.get(volume_id).cloned()
    }

    pub fn find_by_mount_path(&self, mount_path: &str) -> Option<VolumeInfo> {
        let lock = self.0.lock().ok()?;
        lock.values().find(|v| v.mount_path == mount_path).cloned()
    }
}
