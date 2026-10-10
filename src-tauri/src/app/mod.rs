pub(crate) mod background;
pub(crate) mod events;
pub(crate) mod locale;
pub(crate) mod startup;
pub(crate) mod state;
pub(crate) mod tray;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub(crate) fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let startup = startup::initialize(app)?;
            let has_folders = startup.has_folders;
            background::spawn(app.handle().clone(), startup);
            tray::setup(app, has_folders)
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::auth::login,
            crate::commands::auth::logout,
            crate::commands::auth::get_auth_status,
            crate::commands::auth::get_server_url,
            crate::commands::auth::get_current_user_name,
            crate::commands::folders::add_folder,
            crate::commands::folders::get_folders,
            crate::commands::folders::remove_folder,
            crate::commands::sync::start_sync,
            crate::commands::sync::pause_sync,
            crate::commands::sync::resume_sync,
            crate::commands::sync::get_sync_status,
            crate::commands::sync::get_failed_syncs,
            crate::commands::sync::retry_failed_syncs,
            crate::commands::sync::get_upload_parallelism,
            crate::commands::sync::set_upload_parallelism,
            crate::commands::settings::update_locale,
            crate::commands::settings::get_database_recovery_notice
        ])
        .on_window_event(tray::hide_on_close)
        .build(tauri::generate_context!())
        .expect("error while building application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(logger) = app
                    .try_state::<std::sync::Arc<crate::audit::AuditLogger>>()
                    .map(|state| state.inner().clone())
                {
                    let _ = logger.append(crate::audit::AuditEvent::new(
                        crate::audit::AuditEvent::operation_id(),
                        "app.shutdown",
                        crate::audit::Outcome::Success,
                        crate::audit::Severity::Info,
                        None,
                        "shutdown",
                        "Application stopped",
                        serde_json::json!({}),
                    ));
                    let _ = logger.flush();
                }
            }
        });
}
