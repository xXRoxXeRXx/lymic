use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
pub(crate) struct UiLogMessage {
    pub(crate) level: &'static str,
    pub(crate) message: String,
    pub(crate) timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) params: Option<std::collections::HashMap<String, String>>,
}

// Keep UI event names and payload shapes stable for the frontend.
type LogParams = std::collections::HashMap<String, String>;

fn params_from(pairs: &[(&str, &str)]) -> LogParams {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect()
}

pub(crate) fn send_notification_keyed(
    app: &AppHandle,
    title: &str,
    body: &str,
    level: &str,
    key: &str,
    params: LogParams,
) {
    let _ = app.notification().builder().title(title).body(body).show();
    log_to_ui_keyed(app, level, body, key, params);
}

pub(crate) fn log_to_ui_key(app: &AppHandle, level: &str, message: &str, key: &str) {
    log_to_ui_entry(app, level, message, Some(key.to_string()), None);
}

pub(crate) fn log_to_ui_keyed<R: Runtime>(
    app: &AppHandle<R>,
    level: &str,
    message: &str,
    key: &str,
    params: LogParams,
) {
    log_to_ui_entry(app, level, message, Some(key.to_string()), Some(params));
}

pub(crate) fn log_to_ui_folder(
    app: &AppHandle,
    level: &str,
    message: &str,
    key: &str,
    folder: &str,
) {
    log_to_ui_keyed(app, level, message, key, params_from(&[("folder", folder)]));
}

pub(crate) fn log_to_ui_folder_error(
    app: &AppHandle,
    level: &str,
    message: &str,
    key: &str,
    folder: &str,
    error: &str,
) {
    log_to_ui_keyed(
        app,
        level,
        message,
        key,
        params_from(&[("folder", folder), ("error", error)]),
    );
}

pub(crate) fn log_to_ui_file_error(
    app: &AppHandle,
    level: &str,
    message: &str,
    key: &str,
    file: &str,
    error: &str,
) {
    log_to_ui_keyed(
        app,
        level,
        message,
        key,
        params_from(&[("file", file), ("error", error)]),
    );
}

pub(crate) fn log_to_ui_error<R: Runtime>(
    app: &AppHandle<R>,
    level: &str,
    message: &str,
    key: &str,
    error: &str,
) {
    log_to_ui_keyed(app, level, message, key, params_from(&[("error", error)]));
}

fn log_to_ui_entry<R: Runtime>(
    app: &AppHandle<R>,
    level: &str,
    message: &str,
    key: Option<String>,
    params: Option<std::collections::HashMap<String, String>>,
) {
    let payload = UiLogMessage {
        level: match level {
            "SUCCESS" => "SUCCESS",
            "ERROR" => "ERROR",
            "WARN" => "WARN",
            _ => "INFO",
        },
        message: message.to_owned(),
        timestamp: chrono::Local::now().timestamp_millis(),
        key,
        params,
    };
    let _ = app.emit("log-message", payload);
}

pub(crate) fn emit_sync_error(app: &AppHandle, error: &str) {
    let _ = app.emit("sync-error", error);
}
