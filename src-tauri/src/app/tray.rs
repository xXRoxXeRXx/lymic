use crate::app::locale::{backend_translations, LocaleState};
use crate::app::state::{TrayMenuState, TrayTransferStatus};
use crate::sync::progress::TransferStatsEvent;
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
    let locale = app
        .try_state::<LocaleState>()
        .and_then(|state| state.0.lock().ok().map(|locale| locale.clone()))
        .unwrap_or_else(|| "en".to_string());
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

pub(crate) fn setup(app: &tauri::App, has_folders: bool) -> Result<(), Box<dyn std::error::Error>> {
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
                let _ = app.emit("trigger-sync", ());
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
