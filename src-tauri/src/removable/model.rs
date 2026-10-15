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
