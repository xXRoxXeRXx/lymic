#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseRecoveryNoticeData {
    pub(crate) message: String,
    pub(crate) key: String,
    pub(crate) reason: String,
    pub(crate) backup: String,
    pub(crate) count: usize,
}

pub(crate) struct DatabaseRecoveryNotice(
    pub(crate) std::sync::Mutex<Option<DatabaseRecoveryNoticeData>>,
);

pub(crate) struct TrayMenuState(pub(crate) tauri::menu::Menu<tauri::Wry>);

pub(crate) struct TrayTransferStatus(
    pub(crate) std::sync::Mutex<Option<crate::sync::progress::TransferStatsEvent>>,
);

pub(crate) struct TraySyncStatus(pub(crate) std::sync::Mutex<String>);
