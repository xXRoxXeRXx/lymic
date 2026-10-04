use crate::app::locale::{backend_translations, is_supported_locale, LocaleState};
use crate::app::state::{DatabaseRecoveryNotice, TrayMenuState};
use tauri::{menu::MenuItemKind, Manager};

#[tauri::command]
pub(crate) fn get_database_recovery_notice(
    state: tauri::State<'_, DatabaseRecoveryNotice>,
) -> Option<String> {
    state.0.lock().ok().and_then(|mut guard| guard.take())
}

#[tauri::command]
pub(crate) fn update_locale(
    app: tauri::AppHandle,
    state: tauri::State<'_, LocaleState>,
    locale: String,
) -> Result<(), String> {
    if !is_supported_locale(&locale) {
        return Err(format!("Unsupported locale: {}", locale));
    }

    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    *current = locale.clone();

    // Update tray menu labels
    let translations = backend_translations(&locale);
    let tray_menu = app.state::<TrayMenuState>();
    let menu = &tray_menu.0;

    if let Some(MenuItemKind::MenuItem(item)) = menu.get("quit") {
        let _ = item.set_text(&translations.tray_quit);
    }
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("show") {
        let _ = item.set_text(&translations.tray_show);
    }
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("sync") {
        let _ = item.set_text(&translations.tray_sync);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_all_supported_locales_and_translations() {
        assert!(is_supported_locale("en"));
        assert!(is_supported_locale("de"));
        assert!(!is_supported_locale("fr"));
        assert!(!is_supported_locale(""));

        let en = backend_translations("en");
        assert!(!en.tray_quit.is_empty());
        assert!(!en.notification_complete_body.is_empty());

        let de = backend_translations("de");
        assert!(!de.tray_quit.is_empty());
        assert!(!de.notification_complete_body.is_empty());
    }
}
