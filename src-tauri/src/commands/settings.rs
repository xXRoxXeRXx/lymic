use crate::app::locale::{backend_translations, is_supported_locale, LocaleState};
use crate::app::state::{DatabaseRecoveryNotice, TrayMenuState};
use crate::audit::{audit_event, AuditEvent, Outcome, Severity};
use serde_json::json;
use tauri::{menu::MenuItemKind, Manager};

const WEBVIEW_STARTUP_PHASES: &[&str] = &[
    "locale_update_started",
    "locale_update_complete",
    "on_mount_started",
    "update_check_started",
    "update_check_complete",
    "version_load_started",
    "version_load_complete",
    "auth_status_started",
    "auth_status_complete",
    "folders_load_complete",
    "sync_status_complete",
    "autostart_status_complete",
    "upload_parallelism_complete",
    "listeners_complete",
];

fn record_webview_startup_phase(app: &tauri::AppHandle, phase: &str) {
    audit_event(
        app,
        AuditEvent::new(
            AuditEvent::operation_id(),
            "app.webview.startup.phase",
            Outcome::Success,
            Severity::Info,
            None,
            "webview",
            phase,
            json!({ "phase": phase }),
        ),
    );
}

#[tauri::command]
pub(crate) fn record_webview_startup_phase_probe(
    app: tauri::AppHandle,
    phase: String,
) -> Result<(), String> {
    if !WEBVIEW_STARTUP_PHASES.contains(&phase.as_str()) {
        return Err("Unsupported webview startup phase".to_string());
    }
    record_webview_startup_phase(&app, &phase);
    Ok(())
}

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

    record_webview_startup_phase(&app, "locale_update_started");

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
    crate::app::tray::refresh_sync_action(&app);
    crate::app::tray::refresh_transfer_status(&app);

    record_webview_startup_phase(&app, "locale_update_complete");
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
        assert!(!en.tray_pause_sync.is_empty());
        assert!(!en.tray_resume_sync.is_empty());
        assert!(!en.tray_idle_status.is_empty());
        assert!(!en.notification_complete_body.is_empty());

        let de = backend_translations("de");
        assert!(!de.tray_quit.is_empty());
        assert!(!de.tray_pause_sync.is_empty());
        assert!(!de.tray_resume_sync.is_empty());
        assert!(!de.tray_idle_status.is_empty());
        assert!(!de.notification_complete_body.is_empty());
    }
}
