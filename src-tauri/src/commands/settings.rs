use crate::app::locale::{
    backend_translations, is_supported_locale, locale_catalog_entries, LocaleCatalogEntry,
    LocaleState,
};
use crate::app::state::{DatabaseRecoveryNotice, DatabaseRecoveryNoticeData, TrayMenuState};
use crate::audit::{audit_event, AuditEvent, Outcome, Severity};
use serde_json::json;
use tauri::{menu::MenuItemKind, Emitter, Manager};

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

#[tauri::command]
pub(crate) fn get_locale_catalog() -> Vec<LocaleCatalogEntry> {
    locale_catalog_entries()
}

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
) -> Option<DatabaseRecoveryNoticeData> {
    state.0.lock().ok().and_then(|mut guard| guard.take())
}

#[tauri::command]
pub(crate) async fn get_virtual_library_status(
    pool: tauri::State<'_, sqlx::SqlitePool>,
    root: tauri::State<'_, crate::virtual_library::sync_root::SyncRootLifecycle>,
) -> Result<crate::db::VirtualLibraryStatus, String> {
    let mut status = crate::db::get_virtual_library_status(pool.inner())
        .await
        .map_err(|error| error.to_string())?;
    status.connection_status = root.status();
    Ok(status)
}

#[tauri::command]
pub(crate) async fn set_virtual_library_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    root: tauri::State<'_, crate::virtual_library::sync_root::SyncRootLifecycle>,
) -> Result<(), String> {
    let current = crate::db::get_virtual_library_status(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    if enabled {
        crate::db::set_virtual_library_enabled(pool.inner(), true).await?;
        let _ = root.enable(&current.path);
    } else {
        root.disable()?;
        crate::db::set_virtual_library_enabled(pool.inner(), false).await?;
    }
    app.emit(
        "virtual-library-status",
        crate::db::VirtualLibraryStatus {
            enabled,
            path: current.path,
            connection_status: root.status(),
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn get_thumbnail_cache_cap_bytes(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<u64, String> {
    crate::db::get_thumbnail_cache_cap_bytes(pool.inner())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn set_thumbnail_cache_cap_bytes(
    thumbnail_cache_cap_bytes: u64,
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<(), String> {
    crate::db::set_thumbnail_cache_cap_bytes(pool.inner(), thumbnail_cache_cap_bytes).await
}

#[tauri::command]
pub(crate) fn clear_thumbnail_cache(app: tauri::AppHandle) -> Result<(), String> {
    let path = crate::thumbnail_cache::local_thumbnail_cache_dir(&app)?;
    crate::thumbnail_cache::ThumbnailCache::new(path, 0).clear()
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
    // `refresh_sync_action` reads LocaleState again. Release this non-reentrant mutex
    // before updating tray labels, otherwise locale initialization deadlocks at startup.
    drop(current);

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
        let supported = ["en", "de", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru"];
        for locale in supported {
            assert!(
                is_supported_locale(locale),
                "Locale {} should be supported",
                locale
            );
            let trans = backend_translations(locale);
            assert!(!trans.tray_quit.is_empty());
            assert!(!trans.tray_pause_sync.is_empty());
            assert!(!trans.tray_resume_sync.is_empty());
            assert!(!trans.tray_idle_status.is_empty());
            assert!(!trans.notification_complete_body.is_empty());
        }

        assert!(!is_supported_locale("it"));
        assert!(!is_supported_locale("ja"));
        assert!(!is_supported_locale(""));
    }
}
