pub(crate) mod background;
pub(crate) mod events;
pub(crate) mod locale;
pub(crate) mod startup;
pub(crate) mod state;
pub(crate) mod tray;

use std::ffi::{OsStr, OsString};
use tauri::Manager;

const AUTOSTART_ARGUMENT: &str = "--autostart";

fn has_stored_credentials<T, E>(credentials: Result<Option<T>, E>) -> bool {
    matches!(credentials, Ok(Some(_)))
}

fn should_start_in_background(args: &[OsString], has_credentials: bool) -> bool {
    has_credentials && args.iter().any(|arg| arg == OsStr::new(AUTOSTART_ARGUMENT))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub(crate) fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg(AUTOSTART_ARGUMENT)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let args = std::env::args_os().collect::<Vec<_>>();
            let has_credentials = has_stored_credentials(crate::auth::get_credentials());
            let start_in_background = should_start_in_background(&args, has_credentials);

            let startup = startup::initialize(app)?;
            #[cfg(windows)]
            {
                if let Ok(cache_dir) =
                    crate::thumbnail_cache::local_thumbnail_cache_dir(app.handle())
                {
                    crate::thumbnail_pipe::start_server(std::sync::Arc::new(
                        crate::thumbnail_cache::ThumbnailCache::new(cache_dir, 0),
                    ));
                }
            }
            let has_folders = startup.has_folders;
            let recovered_sync = startup.recovered_sync.clone();
            background::spawn(app.handle().clone(), startup);
            startup::record_startup_phase(
                app.handle(),
                "tray_initializing",
                crate::audit::Outcome::Started,
                None,
            );
            tray::setup(app, has_folders, recovered_sync).inspect_err(|error| {
                startup::record_startup_phase(
                    app.handle(),
                    "tray_initialization_failed",
                    crate::audit::Outcome::Failure,
                    Some(error.to_string()),
                );
            })?;
            startup::record_startup_phase(
                app.handle(),
                "tray_ready",
                crate::audit::Outcome::Success,
                None,
            );

            // `setup` runs before the Windows event loop. Scheduling the native window call
            // avoids blocking that loop with `show` or `set_focus` during initialization.
            if !start_in_background {
                let handle = app.handle().clone();
                startup::record_startup_phase(
                    &handle,
                    "main_window_display_queued",
                    crate::audit::Outcome::Started,
                    None,
                );
                handle.clone().run_on_main_thread(move || {
                    startup::record_startup_phase(
                        &handle,
                        "main_window_displaying",
                        crate::audit::Outcome::Started,
                        None,
                    );
                    tray::show_main_window(&handle);
                    startup::record_startup_phase(
                        &handle,
                        "main_window_displayed",
                        crate::audit::Outcome::Success,
                        None,
                    );
                })?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::auth::login,
            crate::commands::auth::logout,
            crate::commands::auth::get_auth_status,
            crate::commands::auth::get_server_url,
            crate::commands::auth::get_current_user_name,
            crate::commands::albums::get_albums,
            crate::commands::albums::create_album,
            crate::commands::folders::add_folder,
            crate::commands::folders::accept_removable_media_folder,
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
            crate::commands::settings::get_locale_catalog,
            crate::commands::settings::update_locale,
            crate::commands::settings::get_virtual_library_status,
            crate::commands::settings::set_virtual_library_enabled,
            crate::commands::settings::get_thumbnail_cache_cap_bytes,
            crate::commands::settings::set_thumbnail_cache_cap_bytes,
            crate::commands::settings::clear_thumbnail_cache,
            crate::commands::settings::clear_virtual_library_cache,
            crate::commands::settings::record_webview_startup_phase_probe,
            crate::commands::settings::get_database_recovery_notice
        ])
        .on_window_event(tray::hide_on_close)
        .build(tauri::generate_context!())
        .expect("error while building application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Ready) {
                startup::record_startup_phase(
                    app,
                    "event_loop_ready",
                    crate::audit::Outcome::Success,
                    None,
                );
            }
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(root) =
                    app.try_state::<crate::virtual_library::sync_root::SyncRootLifecycle>()
                {
                    let _ = root.disable();
                }
                let _ = crate::virtual_library::shell_extension::unregister_for_current_user();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_with_credentials_starts_in_background() {
        assert!(should_start_in_background(
            &[OsString::from("lymic"), OsString::from(AUTOSTART_ARGUMENT)],
            true
        ));
    }

    #[test]
    fn autostart_without_credentials_shows_window() {
        assert!(!should_start_in_background(
            &[OsString::from("lymic"), OsString::from(AUTOSTART_ARGUMENT)],
            false
        ));
    }

    #[test]
    fn manual_launch_with_credentials_shows_window() {
        assert!(!should_start_in_background(
            &[OsString::from("lymic")],
            true
        ));
    }

    #[test]
    fn credential_errors_are_treated_as_not_authenticated() {
        let has_credentials = has_stored_credentials::<(), _>(Err("keyring unavailable"));

        assert!(!has_credentials);
        assert!(!has_stored_credentials::<(), ()>(Ok(None)));
        assert!(!should_start_in_background(
            &[OsString::from("lymic"), OsString::from(AUTOSTART_ARGUMENT)],
            has_credentials
        ));
    }
}
