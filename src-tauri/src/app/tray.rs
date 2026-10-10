use crate::app::locale::{backend_translations, LocaleState};
use crate::app::state::{TrayMenuState, TraySyncStatus, TrayTransferStatus};
use crate::sync::progress::TransferStatsEvent;
use crate::{
    app::events::{emit_sync_error, log_to_ui},
    commands::sync,
    db,
    sync::SyncCoordinator,
};
use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

pub(crate) fn set_sync_enabled(app: &tauri::AppHandle, enabled: bool) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    if let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync") {
        let _ = item.set_enabled(enabled);
    }
}

pub(crate) fn update_sync_action(app: &tauri::AppHandle, snapshot: &db::SyncSnapshot) {
    if let Some(state) = app.try_state::<TraySyncStatus>() {
        *state.0.lock().expect("tray sync status lock poisoned") = snapshot.status.clone();
    }
    apply_sync_action(app, &snapshot.status);
}

pub(crate) fn refresh_sync_action(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<TraySyncStatus>() else {
        return;
    };
    let status = state
        .0
        .lock()
        .expect("tray sync status lock poisoned")
        .clone();
    apply_sync_action(app, &status);
}

fn apply_sync_action(app: &tauri::AppHandle, status: &str) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync") else {
        return;
    };
    let locale = current_locale(app);
    let _ = item.set_text(sync_action_text(status, backend_translations(&locale)));
}

fn current_locale(app: &tauri::AppHandle) -> String {
    app.try_state::<LocaleState>()
        .and_then(|state| state.0.lock().ok().map(|locale| locale.clone()))
        .unwrap_or_else(|| "en".to_string())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Pause,
    Resume,
    Start,
}

pub(crate) fn decide_sync_action(status: &str) -> TrayAction {
    match status {
        "RUNNING" => TrayAction::Pause,
        "PAUSED" => TrayAction::Resume,
        _ => TrayAction::Start,
    }
}

fn sync_action_text<'a>(
    status: &str,
    translations: &'a crate::app::locale::BackendTranslations,
) -> &'a str {
    match status {
        "RUNNING" => &translations.tray_pause_sync,
        "PAUSED" => &translations.tray_resume_sync,
        _ => &translations.tray_sync,
    }
}

pub(crate) fn update_transfer_status(app: &tauri::AppHandle, event: TransferStatsEvent) {
    if let Some(state) = app.try_state::<TrayTransferStatus>() {
        *state.0.lock().expect("tray transfer status lock poisoned") = Some(event.clone());
    }
    apply_transfer_status(app, &event);
}

pub(crate) fn refresh_transfer_status(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<TrayTransferStatus>() else {
        return;
    };
    let event = state
        .0
        .lock()
        .expect("tray transfer status lock poisoned")
        .clone();
    if let Some(event) = event {
        apply_transfer_status(app, &event);
    }
}

fn apply_transfer_status(app: &tauri::AppHandle, event: &TransferStatsEvent) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync-status") else {
        return;
    };
    let visible = event.status == "RUNNING" || event.status == "PAUSED";
    let _ = item.set_enabled(false);
    let text = if visible {
        format_transfer_status(app, event)
    } else {
        String::new()
    };
    let _ = item.set_text(&text);
}

fn format_transfer_status(app: &tauri::AppHandle, event: &TransferStatsEvent) -> String {
    let locale = current_locale(app);
    let translations = backend_translations(&locale);
    let mut parts = vec![format!(
        "{}% | {}: {}",
        if event.total_bytes == 0 {
            0
        } else {
            ((event.completed_bytes as f64 / event.total_bytes as f64) * 100.0).round() as u64
        },
        translations.tray_transferred,
        format_bytes(event.stats.transferred_bytes, &locale),
    )];
    if event.stats.transfer_rate_bytes_per_second > 0.0 {
        let rate = event.stats.transfer_rate_bytes_per_second / 1_000_000.0;
        let rate = if locale == "de" {
            format!("{rate:.1}").replace('.', ",")
        } else {
            format!("{rate:.1}")
        };
        parts.push(format!("{}: {} MB/s", translations.tray_rate, rate));
    }
    if let Some(seconds) = event.stats.estimated_seconds_remaining {
        parts.push(format!(
            "{}: {}",
            translations.tray_eta,
            format_duration(seconds)
        ));
    }
    parts.join(" | ")
}

fn format_bytes(bytes: u64, locale: &str) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        let value = if locale == "de" {
            format!("{value:.1}").replace('.', ",")
        } else {
            format!("{value:.1}")
        };
        format!("{} {}", value, UNITS[index])
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

pub(crate) fn setup(
    app: &tauri::App,
    has_folders: bool,
    recovered_sync: db::SyncSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let translations = backend_translations("en");
    let quit_i = MenuItem::with_id(app, "quit", &translations.tray_quit, true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show", &translations.tray_show, true, None::<&str>)?;
    let sync_i = MenuItem::with_id(
        app,
        "sync",
        &translations.tray_sync,
        has_folders,
        None::<&str>,
    )?;
    let status_i = MenuItem::with_id(app, "sync-status", "", false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status_i, &sync_i, &show_i, &quit_i])?;
    app.handle().manage(TrayMenuState(menu.clone()));
    app.handle()
        .manage(TrayTransferStatus(std::sync::Mutex::new(None)));
    app.handle().manage(TraySyncStatus(std::sync::Mutex::new(
        recovered_sync.status.clone(),
    )));
    update_sync_action(app.handle(), &recovered_sync);
    let icon = app
        .default_window_icon()
        .ok_or("No default window icon configured in tauri.conf.json")?
        .clone();
    let tray_builder = TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => show_main_window(app),
            "sync" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    handle_sync_action(app).await;
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                #[cfg(not(target_os = "macos"))]
                show_main_window(_tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    let tray_builder = tray_builder.show_menu_on_left_click(true);
    #[cfg(not(target_os = "macos"))]
    let tray_builder = tray_builder.show_menu_on_left_click(false);
    let _tray = tray_builder.build(app)?;
    Ok(())
}

async fn handle_sync_action(app: tauri::AppHandle) {
    let pool = app.state::<sqlx::SqlitePool>().inner().clone();
    let coordinator = app.state::<SyncCoordinator>().inner().0.clone();
    let result = async {
        let snapshot = db::get_sync_snapshot(&pool)
            .await
            .map_err(|error| error.to_string())?;
        match decide_sync_action(&snapshot.status) {
            TrayAction::Pause => sync::pause_sync_inner(&app, &pool, &coordinator)
                .await
                .map(|_| ()),
            TrayAction::Resume => sync::resume_sync_inner(&app, &pool, &coordinator)
                .await
                .map(|_| ()),
            TrayAction::Start => app
                .emit("trigger-sync", ())
                .map_err(|error| error.to_string()),
        }
    }
    .await;

    if let Err(error) = result {
        log_to_ui(
            &app,
            "ERROR",
            &format!("Tray synchronization action failed: {error}"),
        );
        emit_sync_error(&app, &error);
    }
    match db::get_sync_snapshot(&pool).await {
        Ok(snapshot) => update_sync_action(&app, &snapshot),
        Err(error) => log_to_ui(
            &app,
            "ERROR",
            &format!("Could not refresh tray sync state: {error}"),
        ),
    }
}

pub(crate) fn hide_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let _ = window.hide();
        api.prevent_close();
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_action_text_follows_persisted_status() {
        let translations = backend_translations("en");
        assert_eq!(sync_action_text("IDLE", translations), "Sync Now");
        assert_eq!(sync_action_text("RUNNING", translations), "Pause Sync");
        assert_eq!(sync_action_text("PAUSED", translations), "Resume Sync");
    }

    #[test]
    fn sync_action_text_is_localized() {
        let translations = backend_translations("de");
        assert_eq!(
            sync_action_text("RUNNING", translations),
            "Synchronisierung pausieren"
        );
        assert_eq!(
            sync_action_text("PAUSED", translations),
            "Synchronisierung fortsetzen"
        );
    }

    #[test]
    fn tray_action_dispatch_follows_persisted_status() {
        assert_eq!(decide_sync_action("RUNNING"), TrayAction::Pause);
        assert_eq!(decide_sync_action("PAUSED"), TrayAction::Resume);
        assert_eq!(decide_sync_action("IDLE"), TrayAction::Start);
    }
}
