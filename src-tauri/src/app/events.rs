use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

// Keep UI event names and payload shapes stable for the frontend.
pub(crate) fn send_notification(app: &AppHandle, title: &str, body: &str, level: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
    log_to_ui(app, level, body);
}

pub(crate) fn log_to_ui(app: &AppHandle, level: &str, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_line = format!("[{}] [{}] {}", timestamp, level, message);
    let _ = app.emit("log-message", log_line);
}

pub(crate) fn emit_sync_error(app: &AppHandle, error: &str) {
    let _ = app.emit("sync-error", error);
}
