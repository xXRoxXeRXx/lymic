#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub volume_id: String,
    pub mount_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeEvent {
    Arrival(VolumeInfo),
    Removal(String),
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

    pub fn remove(&self, id_or_path: &str) {
        if let Ok(mut lock) = self.0.lock() {
            lock.retain(|k, v| k != id_or_path && v.mount_path != id_or_path);
        }
    }

    pub fn get(&self, volume_id: &str) -> Option<VolumeInfo> {
        self.0.lock().ok()?.get(volume_id).cloned()
    }

    pub fn find_by_id_or_path(&self, id_or_path: &str) -> Option<VolumeInfo> {
        let lock = self.0.lock().ok()?;
        lock.values()
            .find(|v| v.volume_id == id_or_path || v.mount_path == id_or_path)
            .cloned()
    }
}
