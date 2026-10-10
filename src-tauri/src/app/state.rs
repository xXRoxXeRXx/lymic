pub(crate) struct DatabaseRecoveryNotice(pub(crate) std::sync::Mutex<Option<String>>);

pub(crate) struct TrayMenuState(pub(crate) tauri::menu::Menu<tauri::Wry>);

pub(crate) struct TrayTransferStatus(
    pub(crate) std::sync::Mutex<Option<crate::sync::progress::TransferStatsEvent>>,
);

pub(crate) struct TraySyncStatus(pub(crate) std::sync::Mutex<String>);
