pub(crate) struct DatabaseRecoveryNotice(pub(crate) std::sync::Mutex<Option<String>>);

pub(crate) struct TrayMenuState(pub(crate) tauri::menu::Menu<tauri::Wry>);
