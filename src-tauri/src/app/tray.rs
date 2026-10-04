use crate::app::locale::backend_translations;
use crate::app::state::TrayMenuState;
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
    let menu = Menu::with_items(app, &[&sync_i, &show_i, &quit_i])?;
    app.handle().manage(TrayMenuState(menu.clone()));
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
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                #[cfg(not(target_os = "macos"))]
                show_main_window(tray.app_handle());
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
