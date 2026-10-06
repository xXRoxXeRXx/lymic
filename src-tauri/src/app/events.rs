use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, serde::Serialize)]
pub(crate) struct UiLogMessage {
    level: &'static str,
    message: String,
    timestamp: i64,
}

// Keep UI event names and payload shapes stable for the frontend.
pub(crate) fn send_notification(app: &AppHandle, title: &str, body: &str, level: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
    log_to_ui(app, level, body);
}

pub(crate) fn log_to_ui(app: &AppHandle, level: &str, message: &str) {
    let payload = UiLogMessage {
        level: match level {
            "SUCCESS" => "SUCCESS",
            "ERROR" => "ERROR",
            "WARN" => "WARN",
            _ => "INFO",
        },
        message: message.to_owned(),
        timestamp: chrono::Local::now().timestamp_millis(),
    };
    let _ = app.emit("log-message", payload);
}

pub(crate) fn emit_sync_error(app: &AppHandle, error: &str) {
    let _ = app.emit("sync-error", error);
}
