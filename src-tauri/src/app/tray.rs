use crate::app::locale::{backend_translations, LocaleState};
use crate::app::state::{TrayMenuState, TraySyncStatus, TrayTransferStatus};
use crate::sync::progress::TransferStatsEvent;
use crate::{
    app::events::{emit_sync_error, log_to_ui_error},
    db,
    sync::{SyncRuntime, Synchronization},
};
use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

pub(crate) fn set_sync_enabled(app: &tauri::AppHandle, enabled: bool) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    if let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync") {
        let _ = item.set_enabled(enabled);
    }
}

pub(crate) fn update_sync_action<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    snapshot: &db::SyncSnapshot,
) {
    if let Some(state) = app.try_state::<TraySyncStatus>() {
        *state.0.lock().expect("tray sync status lock poisoned") = snapshot.status;
    }
    apply_sync_action(app, snapshot.status);
}

pub(crate) fn refresh_sync_action(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<TraySyncStatus>() else {
        return;
    };
    let status = *state.0.lock().expect("tray sync status lock poisoned");
    apply_sync_action(app, status);
}

fn apply_sync_action<R: tauri::Runtime>(app: &tauri::AppHandle<R>, status: db::SyncJobStatus) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync") else {
        return;
    };
    let locale = current_locale(app);
    let _ = item.set_text(sync_action_text(status, backend_translations(&locale)));
}

fn current_locale<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    app.try_state::<LocaleState>()
        .and_then(|state| state.0.lock().ok().map(|locale| locale.clone()))
        .unwrap_or_else(|| "en".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Pause,
    Resume,
    Start,
}

impl TrayAction {
    pub(crate) fn from_status(status: db::SyncJobStatus) -> Self {
        match status {
            db::SyncJobStatus::Running => Self::Pause,
            db::SyncJobStatus::Paused => Self::Resume,
            db::SyncJobStatus::Idle | db::SyncJobStatus::Failed => Self::Start,
        }
    }

    fn label(self, translations: &crate::app::locale::BackendTranslations) -> &str {
        match self {
            Self::Pause => &translations.tray_pause_sync,
            Self::Resume => &translations.tray_resume_sync,
            Self::Start => &translations.tray_sync,
        }
    }
}

pub(crate) fn decide_sync_action(status: db::SyncJobStatus) -> TrayAction {
    TrayAction::from_status(status)
}

fn sync_action_text(
    status: db::SyncJobStatus,
    translations: &crate::app::locale::BackendTranslations,
) -> &str {
    TrayAction::from_status(status).label(translations)
}

pub(crate) fn update_transfer_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: TransferStatsEvent,
) {
    if let Some(state) = app.try_state::<TrayTransferStatus>() {
        *state.0.lock().expect("tray transfer status lock poisoned") = Some(event.clone());
    }
    apply_transfer_status(app, Some(&event));
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
    apply_transfer_status(app, event.as_ref());
}

fn apply_transfer_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: Option<&TransferStatsEvent>,
) {
    let text = transfer_status_label(event, &current_locale(app));
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync-status") else {
        return;
    };
    let _ = item.set_enabled(false);
    let _ = item.set_text(&text);
}

fn transfer_status_label(event: Option<&TransferStatsEvent>, locale: &str) -> String {
    match event {
        Some(event)
            if matches!(
                event.status,
                db::SyncJobStatus::Running | db::SyncJobStatus::Paused
            ) =>
        {
            format_transfer_status(event, locale)
        }
        _ => idle_transfer_status(locale),
    }
}

fn idle_transfer_status(locale: &str) -> String {
    backend_translations(locale).tray_idle_status.clone()
}

fn format_transfer_status(event: &TransferStatsEvent, locale: &str) -> String {
    let translations = backend_translations(locale);
    let mut parts = vec![format!(
        "{}%",
        crate::sync::progress::calculate_percent(event.completed_bytes, event.total_bytes)
    )];
    if let Some(seconds) = event.stats.estimated_seconds_remaining {
        parts.push(format!(
            "{}: {}",
            translations.tray_eta,
            format_duration(seconds)
        ));
    }
    parts.join(" | ")
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
    let status_i = MenuItem::with_id(
        app,
        "sync-status",
        &translations.tray_idle_status,
        false,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&status_i, &sync_i, &show_i, &quit_i])?;
    app.handle().manage(TrayMenuState(menu.clone()));
    app.handle()
        .manage(TrayTransferStatus(std::sync::Mutex::new(None)));
    app.handle()
        .manage(TraySyncStatus(std::sync::Mutex::new(recovered_sync.status)));
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
    let runtime = app.state::<SyncRuntime>().inner().clone();
    let result = async {
        let snapshot = runtime.status().await.map_err(|error| error.to_string())?;
        let sync = Synchronization::new(&app, &runtime);
        match decide_sync_action(snapshot.status) {
            TrayAction::Pause => sync.pause().await.map(|_| ()),
            TrayAction::Resume => sync.resume().await.map(|_| ()),
            TrayAction::Start => sync.start().await.map(|_| ()),
        }
    }
    .await;

    if let Err(error) = result {
        log_to_ui_error(
            &app,
            "ERROR",
            &tray_sync_action_error_message(&error),
            "log_tray_sync_action_failed",
            &error,
        );
        emit_sync_error(&app, &error);
    }
    match runtime.status().await {
        Ok(snapshot) => update_sync_action(&app, &snapshot),
        Err(error) => log_to_ui_error(
            &app,
            "ERROR",
            &format!("Could not refresh tray sync state: {error}"),
            "log_tray_refresh_failed",
            &error.to_string(),
        ),
    }
}

fn tray_sync_action_error_message(error: &str) -> String {
    format!("Tray synchronization action failed: {error}")
}

pub(crate) fn hide_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let _ = window.hide();
        api.prevent_close();
    }
}

pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::progress::TransferStatsSnapshot;

    fn transfer_event(status: db::SyncJobStatus, eta: Option<u64>) -> TransferStatsEvent {
        TransferStatsEvent {
            stats: TransferStatsSnapshot {
                transferred_bytes: 42_000_000,
                transfer_rate_bytes_per_second: 12_500_000.0,
                estimated_seconds_remaining: eta,
            },
            status,
            total_bytes: 1_000,
            completed_bytes: 420,
        }
    }

    #[test]
    fn sync_action_text_follows_persisted_status() {
        let translations = backend_translations("en");
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Idle, translations),
            "Sync Now"
        );
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Running, translations),
            "Pause Sync"
        );
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Paused, translations),
            "Resume Sync"
        );
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Failed, translations),
            "Sync Now"
        );
    }

    #[test]
    fn sync_action_text_is_localized() {
        let translations = backend_translations("de");
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Running, translations),
            "Synchronisierung pausieren"
        );
        assert_eq!(
            sync_action_text(db::SyncJobStatus::Paused, translations),
            "Synchronisierung fortsetzen"
        );
    }

    #[test]
    fn tray_action_dispatch_follows_persisted_status() {
        assert_eq!(
            decide_sync_action(db::SyncJobStatus::Running),
            TrayAction::Pause
        );
        assert_eq!(
            decide_sync_action(db::SyncJobStatus::Paused),
            TrayAction::Resume
        );
        assert_eq!(
            decide_sync_action(db::SyncJobStatus::Idle),
            TrayAction::Start
        );
        assert_eq!(
            decide_sync_action(db::SyncJobStatus::Failed),
            TrayAction::Start
        );
    }

    #[test]
    fn tray_action_failure_keeps_the_underlying_error_visible() {
        assert_eq!(
            tray_sync_action_error_message("database unavailable"),
            "Tray synchronization action failed: database unavailable"
        );
    }

    #[test]
    fn idle_transfer_status_is_localized() {
        assert_eq!(idle_transfer_status("en"), "No transfer active");
        assert_eq!(idle_transfer_status("de"), "Keine Übertragung aktiv");
    }

    #[test]
    fn idle_event_and_empty_state_yield_idle_label() {
        let event = transfer_event(db::SyncJobStatus::Idle, None);

        assert_eq!(
            transfer_status_label(Some(&event), "en"),
            "No transfer active"
        );
        assert_eq!(transfer_status_label(None, "de"), "Keine Übertragung aktiv");
    }

    #[test]
    fn active_transfer_status_shows_percentage_and_localized_eta_only() {
        let event = transfer_event(db::SyncJobStatus::Running, Some(192));

        let text = format_transfer_status(&event, "de");

        assert_eq!(text, "42% | Restzeit: 3m 12s");
        assert!(!text.contains("42.0"));
        assert!(!text.contains("MB/s"));
        assert!(!text.contains("Rate"));
    }

    #[test]
    fn active_transfer_status_without_eta_shows_percentage_only() {
        let event = transfer_event(db::SyncJobStatus::Running, None);

        assert_eq!(format_transfer_status(&event, "en"), "42%");
    }

    #[test]
    fn paused_transfer_status_without_eta_shows_percentage_only() {
        let event = transfer_event(db::SyncJobStatus::Paused, None);

        assert_eq!(format_transfer_status(&event, "en"), "42%");
    }

    #[test]
    fn transfer_status_preserves_zero_eta_and_zero_total_behavior() {
        let mut event = transfer_event(db::SyncJobStatus::Running, Some(0));
        event.total_bytes = 0;
        event.completed_bytes = 0;

        assert_eq!(format_transfer_status(&event, "en"), "0% | ETA: 0m 00s");
    }
}
