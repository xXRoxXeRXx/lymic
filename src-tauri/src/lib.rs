mod audit;
mod auth;
mod db;
mod sync;
mod watcher;

use audit::{audit_event, audit_safe_error, upload_details, SyncAuditContext};
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

struct SyncState(AtomicBool);
struct SyncCoordinator(std::sync::Arc<tokio::sync::Mutex<()>>);
struct LocaleState(std::sync::Mutex<String>);
struct DatabaseRecoveryNotice(std::sync::Mutex<Option<String>>);
struct TrayMenuState(Menu<tauri::Wry>);

fn set_tray_sync_enabled(app: &tauri::AppHandle, enabled: bool) {
    let Some(tray_menu) = app.try_state::<TrayMenuState>() else {
        return;
    };

    if let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get("sync") {
        let _ = item.set_enabled(enabled);
    }
}

fn ensure_sync_has_folders(folders: &[db::WatchedFolder]) -> Result<(), String> {
    if folders.is_empty() {
        return Err("Add at least one folder before starting a sync.".to_string());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct BackendTranslations {
    tray_quit: String,
    tray_show: String,
    tray_sync: String,
    notification_complete_title: String,
    notification_complete_body: String,
}

fn is_supported_locale(locale: &str) -> bool {
    matches!(locale, "en" | "de")
}

// The backend consumes its strings from the frontend's translation catalog so the
// tray and notifications cannot drift from the application language.
fn backend_translations(locale: &str) -> &'static BackendTranslations {
    static EN_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static DE_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();

    match locale {
        "de" => DE_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../src/lib/i18n/de.json"))
                .expect("German translations must be valid")
        }),
        _ => EN_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../src/lib/i18n/en.json"))
                .expect("English translations must be valid")
        }),
    }
}

const BULK_CHECK_MAX_ATTEMPTS: usize = 3;
const BULK_CHECK_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
const MAX_STABLE_HASH_ATTEMPTS: usize = 3;
const _: () = assert!(BULK_CHECK_MAX_ATTEMPTS > 0);
const _: () = assert!(MAX_STABLE_HASH_ATTEMPTS > 0);

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SyncSummary {
    processed: usize,
    uploaded: usize,
    failed: usize,
}

// Emits completion when a sync scope exits, including early returns and errors.
struct SyncIdleEmitter(tauri::AppHandle);

impl Drop for SyncIdleEmitter {
    fn drop(&mut self) {
        let _ = self.0.emit("sync-idle", ());
    }
}

// ---------------------------------------------------------------------------
// Supported Immich image and video extensions for sync. Defined once and reused
// in both start_sync and the watcher path. Keep this aligned with Immich's
// server/src/utils/mime-types.ts getSupportedFileExtensions().
// ---------------------------------------------------------------------------
const MEDIA_EXTENSIONS: &[&str] = &[
    "3fr", "3gp", "3gpp", "ari", "arw", "avif", "avi", "bmp", "cap", "cin", "cr2", "cr3", "crw",
    "dcr", "dng", "erf", "fff", "flv", "gif", "heic", "heif", "hif", "iiq", "insp", "jfif", "jp2",
    "jpe", "jpeg", "jpg", "jxl", "k25", "kdc", "m2t", "m2ts", "m4v", "mkv", "mov", "mp4", "mpe",
    "mpeg", "mpg", "mpo", "mrw", "mts", "mxf", "nef", "nrw", "orf", "ori", "pef", "png", "psd",
    "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "svg", "tif", "tiff", "ts", "vob", "webm",
    "webp", "wmv", "x3f",
];

fn is_media_file(path: &std::path::Path) -> bool {
    // eq_ignore_ascii_case avoids a heap allocation per file.
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|ext| e.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

#[derive(Debug)]
struct Asset {
    path: String,
    size: u64,
    mtime: i64,
}

#[derive(Debug)]
struct FileFailure {
    path: String,
    error: String,
}

#[derive(Debug, Default)]
struct ScanResult {
    files: Vec<Asset>,
    failures: Vec<FileFailure>,
    overlapping_folders: Vec<(PathBuf, PathBuf)>,
}

fn normalize_folder_path(path: &Path) -> PathBuf {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    // canonicalize resolves aliases and symbolic links for available folders. Offline folders
    // remain supported, so use an absolute lexical path until they become available.
    let mut normalized_path = PathBuf::new();
    for component in absolute_path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized_path.pop();
            }
            component => normalized_path.push(component.as_os_str()),
        }
    }

    let canonical_path = std::fs::canonicalize(&normalized_path).unwrap_or(normalized_path);
    remove_windows_verbatim_prefix(canonical_path)
}

#[cfg(windows)]
fn remove_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let path_string = path.to_string_lossy();
    if let Some(unc_path) = path_string.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", unc_path))
    } else if let Some(disk_path) = path_string.strip_prefix(r"\\?\") {
        if disk_path
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && disk_path.as_bytes().get(1) == Some(&b':')
        {
            PathBuf::from(disk_path)
        } else {
            path
        }
    } else {
        path
    }
}

#[cfg(not(windows))]
fn remove_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

fn path_component_eq(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str().to_string_lossy().to_lowercase()
            == right.as_os_str().to_string_lossy().to_lowercase()
    }

    #[cfg(not(windows))]
    {
        left == right
    }
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    let mut path_components = path.components();
    parent
        .components()
        .all(|parent_component| match path_components.next() {
            Some(path_component) => path_component_eq(path_component, parent_component),
            None => false,
        })
}

fn overlapping_folder_paths(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<(PathBuf, PathBuf)>) {
    let mut paths = paths
        .into_iter()
        .map(|path| normalize_folder_path(&path))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.components().count());

    let mut folders = Vec::new();
    let mut overlaps = Vec::new();
    for path in paths {
        if let Some(parent) = folders
            .iter()
            .find(|folder: &&PathBuf| path_is_within(&path, folder))
        {
            overlaps.push((path, parent.clone()));
        } else {
            folders.push(path);
        }
    }
    (folders, overlaps)
}

fn metadata_mtime(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

fn file_version(path: &str) -> std::io::Result<(u64, i64)> {
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.len(), metadata_mtime(&metadata)))
}

fn calculate_stable_checksums(path: &str) -> std::io::Result<(sync::Checksums, u64, i64)> {
    // Refresh the scan snapshot before hashing to avoid a redundant hash when it is stale.
    let mut version = file_version(path)?;

    for _ in 0..MAX_STABLE_HASH_ATTEMPTS {
        let checksums = sync::calculate_checksums(path)?;
        let current_version = file_version(path)?;
        if current_version == version {
            return Ok((checksums, current_version.0, current_version.1));
        }
        version = current_version;
    }

    Err(std::io::Error::other("File changed while hashing"))
}

fn scan_folder_for_media(path: &std::path::Path) -> ScanResult {
    let mut result = ScanResult::default();
    for entry in walkdir::WalkDir::new(path) {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_media_file(entry.path()) => {
                let file_path = entry.path().to_string_lossy().to_string();
                match entry.metadata() {
                    Ok(metadata) => result.files.push(Asset {
                        path: file_path,
                        size: metadata.len(),
                        mtime: metadata_mtime(&metadata),
                    }),
                    Err(error) => result.failures.push(FileFailure {
                        path: file_path,
                        error: format!("Could not read file metadata: {}", error),
                    }),
                }
            }
            Ok(_) => {}
            Err(error) => result.failures.push(FileFailure {
                path: error.path().unwrap_or(path).to_string_lossy().to_string(),
                error: format!("Could not scan path: {}", error),
            }),
        }
    }
    result
}

async fn scan_folders_for_media(paths: Vec<std::path::PathBuf>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        let (paths, overlapping_folders) = overlapping_folder_paths(paths);
        let mut seen_files = HashSet::new();
        for path in paths {
            let scan_result = scan_folder_for_media(&path);
            for asset in scan_result.files {
                if seen_files.insert(PathBuf::from(&asset.path)) {
                    result.files.push(asset);
                }
            }
            result.failures.extend(scan_result.failures);
        }
        result.overlapping_folders = overlapping_folders;
        result
    })
    .await
    .map_err(|e| format!("Media scan task failed: {}", e))
}

async fn scan_paths_for_media(paths: Vec<std::path::PathBuf>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            if !is_media_file(&path) {
                continue;
            }

            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => result.files.push(Asset {
                    path: path.to_string_lossy().to_string(),
                    size: metadata.len(),
                    mtime: metadata_mtime(&metadata),
                }),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => result.failures.push(FileFailure {
                    path: path.to_string_lossy().to_string(),
                    error: format!("Could not read file metadata: {}", error),
                }),
            }
        }
        result
    })
    .await
    .map_err(|e| format!("Media path scan task failed: {}", e))
}

async fn add_event_paths(
    files_buffer: &mut std::collections::HashMap<String, Asset>,
    failures_buffer: &mut std::collections::HashMap<String, FileFailure>,
    event: notify::Event,
) -> Result<(), String> {
    // Determining whether an event path is a directory can access slow or offline storage.
    let (directories, files) = tokio::task::spawn_blocking(move || {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for path in event.paths {
            if path.is_dir() {
                directories.push(path);
            } else if is_media_file(&path) {
                files.push(path);
            }
        }
        (directories, files)
    })
    .await
    .map_err(|error| format!("Could not classify watcher paths: {}", error))?;

    if directories.is_empty() && files.is_empty() {
        return Ok(());
    }

    let (folder_scan, file_scan) = tokio::try_join!(
        scan_folders_for_media(directories),
        scan_paths_for_media(files),
    )?;
    for scan_result in [folder_scan, file_scan] {
        for asset in scan_result.files {
            files_buffer.insert(asset.path.clone(), asset);
        }
        for failure in scan_result.failures {
            failures_buffer.insert(failure.path.clone(), failure);
        }
    }
    Ok(())
}

// accept a log level so this helper composes correctly if ever
//           reused for error or warning notifications.
fn send_notification(app: &tauri::AppHandle, title: &str, body: &str, level: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
    log_to_ui(app, level, body);
}

fn log_to_ui(app: &tauri::AppHandle, level: &str, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_line = format!("[{}] [{}] {}", timestamp, level, message);
    let _ = app.emit("log-message", log_line);
}

fn log_overlapping_folders(app: &tauri::AppHandle, scan_result: &ScanResult) {
    for (folder, parent) in &scan_result.overlapping_folders {
        log_to_ui(
            app,
            "WARN",
            &format!(
                "Skipping overlapping watched folder '{}' because '{}' is already scanned.",
                folder.display(),
                parent.display()
            ),
        );
    }
}

fn emit_sync_error(app: &tauri::AppHandle, error: &str) {
    let _ = app.emit("sync-error", error);
}

fn create_authenticated_client(
    app: &tauri::AppHandle,
    credentials: auth::AuthConfig,
) -> Option<std::sync::Arc<sync::ImmichClient>> {
    match sync::ImmichClient::new(credentials.server_url, credentials.api_key) {
        Ok(client) => Some(std::sync::Arc::new(client)),
        Err(error) => {
            log_to_ui(
                app,
                "ERROR",
                &format!("Invalid server configuration: {}", error),
            );
            None
        }
    }
}

// (security): api_key is intentionally kept out of all log and error messages below.
#[tauri::command]
async fn login(app: tauri::AppHandle, server_url: String, api_key: String) -> Result<(), String> {
    log_to_ui(&app, "INFO", "Attempting to connect to server");
    let operation_id = audit::AuditEvent::operation_id();
    let actor_id = Some(audit::actor_id(&server_url, &api_key));
    audit_event(
        &app,
        audit::AuditEvent::new(
            &operation_id,
            "auth.login",
            audit::Outcome::Started,
            audit::Severity::Info,
            actor_id.clone(),
            "command",
            "Login started",
            json!({}),
        ),
    );

    // Build a temporary ImmichClient purely to reuse its URL normalisation and
    // pooled reqwest Client. The client is discarded after the connection test.
    let result = async {
        let temp_client = sync::ImmichClient::new(server_url.clone(), api_key.clone())?;
        temp_client.validate_connection().await?;
        auth::store_credentials(&server_url, &api_key)
    }
    .await;
    audit_event(
        &app,
        audit::AuditEvent::new(
            &operation_id,
            "auth.login",
            if result.is_ok() {
                audit::Outcome::Success
            } else {
                audit::Outcome::Failure
            },
            if result.is_ok() {
                audit::Severity::Info
            } else {
                audit::Severity::Error
            },
            actor_id,
            "command",
            if result.is_ok() {
                "Login completed"
            } else {
                "Login failed"
            },
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, Some(&server_url), Some(&api_key))) }),
        ),
    );
    result
}

#[tauri::command]
async fn logout(app: tauri::AppHandle) -> Result<(), String> {
    let operation_id = audit::AuditEvent::operation_id();
    let actor_id = auth::get_credentials()
        .ok()
        .flatten()
        .map(|credentials| audit::actor_id(&credentials.server_url, &credentials.api_key));
    let result = auth::delete_credentials();
    audit_event(
        &app,
        audit::AuditEvent::new(
            operation_id,
            "auth.logout",
            if result.is_ok() {
                audit::Outcome::Success
            } else {
                audit::Outcome::Failure
            },
            if result.is_ok() {
                audit::Severity::Info
            } else {
                audit::Severity::Error
            },
            actor_id,
            "command",
            if result.is_ok() {
                "Logout completed"
            } else {
                "Logout failed"
            },
            json!({ "failure_reason": result.as_ref().err().map(|error| audit_safe_error(error, None, None)) }),
        ),
    );
    result
}

#[tauri::command]
async fn get_auth_status() -> Result<bool, String> {
    Ok(auth::get_credentials()?.is_some())
}

#[tauri::command]
async fn get_server_url() -> Result<String, String> {
    if let Some(creds) = auth::get_credentials()? {
        Ok(creds.server_url)
    } else {
        Ok("".to_string())
    }
}

#[tauri::command]
async fn get_current_user_name() -> Result<String, String> {
    let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
    let client = sync::ImmichClient::new(credentials.server_url, credentials.api_key)?;
    client.current_user().await
}

#[tauri::command]
fn get_database_recovery_notice(state: tauri::State<'_, DatabaseRecoveryNotice>) -> Option<String> {
    state.0.lock().ok().and_then(|mut guard| guard.take())
}

#[tauri::command]
fn update_locale(
    app: tauri::AppHandle,
    state: tauri::State<'_, LocaleState>,
    locale: String,
) -> Result<(), String> {
    if !is_supported_locale(&locale) {
        return Err(format!("Unsupported locale: {}", locale));
    }

    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    *current = locale.clone();

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
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("sync") {
        let _ = item.set_text(&translations.tray_sync);
    }

    Ok(())
}

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    path: String,
) -> Result<i64, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|error| error.to_string())?;
    let path_for_normalization = PathBuf::from(&path);
    let existing_paths = folders
        .into_iter()
        .map(|folder| PathBuf::from(folder.path))
        .collect::<Vec<_>>();
    let (normalized_path, conflict) = tokio::task::spawn_blocking(move || {
        let normalized_path = normalize_folder_path(&path_for_normalization);
        let conflict = existing_paths
            .into_iter()
            .map(|path| normalize_folder_path(&path))
            .find_map(|existing_path| {
                let is_duplicate = path_is_within(&normalized_path, &existing_path)
                    && path_is_within(&existing_path, &normalized_path);
                if is_duplicate
                    || path_is_within(&normalized_path, &existing_path)
                    || path_is_within(&existing_path, &normalized_path)
                {
                    Some((existing_path, is_duplicate))
                } else {
                    None
                }
            });
        (normalized_path, conflict)
    })
    .await
    .map_err(|error| format!("Could not normalize folder '{}': {}", path, error))?;
    let normalized_path_string = normalized_path.to_string_lossy().to_string();

    if let Some((existing_path, is_duplicate)) = conflict {
        let message = if is_duplicate {
            format!(
                "Folder '{}' is already being watched.",
                normalized_path.display()
            )
        } else {
            format!(
                "Folder '{}' overlaps with already watched folder '{}'. Remove one of the folders before adding it.",
                normalized_path.display(),
                existing_path.display()
            )
        };
        return Err(message);
    }

    let path = normalized_path_string;
    let path_for_check = PathBuf::from(&path);
    let is_available = match tokio::task::spawn_blocking(move || {
        std::fs::metadata(path_for_check).map(|metadata| metadata.is_dir())
    })
    .await
    {
        Ok(Ok(true)) => true,
        Ok(Ok(false)) => return Err(format!("Path '{}' is not a directory.", path)),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
        Ok(Err(error)) => return Err(format!("Could not access folder '{}': {}", path, error)),
        Err(error) => {
            return Err(format!("Could not check folder '{}': {}", path, error));
        }
    };

    let registration = if is_available {
        let mut w = watcher.lock().await;
        w.reconcile_path(&path, true)
            .map_err(|error| format!("Could not watch '{}': {}", path, error))?
    } else {
        watcher::WatchRegistration::PathUnavailable
    };

    let id = match db::add_folder(pool.inner(), &path).await {
        Ok(id) => id,
        Err(error) => {
            if registration == watcher::WatchRegistration::Registered {
                let mut w = watcher.lock().await;
                if let Err(unwatch_error) = watcher::unwatch_path(&mut w, &path) {
                    log_to_ui(
                        &app,
                        "ERROR",
                        &format!(
                            "Could not undo watcher registration for '{}': {}",
                            path, unwatch_error
                        ),
                    );
                }
            }
            return Err(error.to_string());
        }
    };

    // The persisted folder is the source of truth, even when its watcher is deferred.
    set_tray_sync_enabled(&app, true);

    if registration == watcher::WatchRegistration::PathUnavailable {
        log_to_ui(
            &app,
            "WARN",
            &format!(
                "Folder '{}' does not exist; watching deferred until it appears.",
                path
            ),
        );
    }

    // An offline folder will be scanned when the deferred-watch task registers it.
    if is_available {
        let pool_inner = pool.inner().clone();
        let path_clone = path.clone();
        let app_for_scan = app.clone();
        tauri::async_runtime::spawn(async move {
            let scan_result = match scan_folders_for_media(vec![PathBuf::from(path_clone)]).await {
                Ok(result) => result,
                Err(error) => {
                    log_to_ui(&app_for_scan, "ERROR", &error);
                    return;
                }
            };
            sync_scan_result_if_authenticated(app_for_scan, pool_inner, scan_result, "folder_add")
                .await;
        });
    }

    audit_event(
        &app,
        audit::AuditEvent::new(
            audit::AuditEvent::operation_id(),
            "folder.added",
            audit::Outcome::Success,
            audit::Severity::Info,
            None,
            "command",
            "Watched folder added",
            json!({ "folder_id": id, "local_path": path }),
        ),
    );
    Ok(id)
}

#[tauri::command]
async fn get_folders(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::WatchedFolder>, String> {
    db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<watcher::WatcherState>>,
    id: i64,
) -> Result<(), String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let path = folder.path.clone();
        {
            let mut w = watcher.lock().await;
            if let Err(e) = watcher::unwatch_path(&mut w, &path) {
                log_to_ui(
                    &app,
                    "WARN",
                    &format!("Failed to unwatch '{}': {}", path, e),
                );
                return Err(format!("Failed to unwatch '{}': {}", path, e));
            }
        }

        if let Err(error) = db::remove_folder(pool.inner(), id).await {
            let path_for_check = PathBuf::from(&path);
            let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
                .await
                .unwrap_or(false);
            let mut w = watcher.lock().await;
            if let Err(rewatch_error) = w.reconcile_path(&path, is_available) {
                log_to_ui(
                    &app,
                    "ERROR",
                    &format!(
                        "Could not restore watcher registration for '{}': {}",
                        path, rewatch_error
                    ),
                );
            }
            return Err(error.to_string());
        }
    } else {
        db::remove_folder(pool.inner(), id)
            .await
            .map_err(|e| e.to_string())?;
    }
    match db::get_folders(pool.inner()).await {
        Ok(folders) => set_tray_sync_enabled(&app, !folders.is_empty()),
        Err(error) => log_to_ui(
            &app,
            "ERROR",
            &format!(
                "Folder was removed, but could not refresh tray sync availability: {}",
                error
            ),
        ),
    }
    audit_event(
        &app,
        audit::AuditEvent::new(
            audit::AuditEvent::operation_id(),
            "folder.removed",
            audit::Outcome::Success,
            audit::Severity::Info,
            None,
            "command",
            "Watched folder removed",
            json!({ "folder_id": id }),
        ),
    );
    Ok(())
}

async fn run_sync_pipeline(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: std::sync::Arc<sync::ImmichClient>,
    scan_result: ScanResult,
    is_auto: bool,
    coordinator: std::sync::Arc<tokio::sync::Mutex<()>>,
    audit_context: SyncAuditContext,
) -> Result<SyncSummary, String> {
    // Auto-sync has no command response, so it reports completion through an event.
    let _idle_emitter = is_auto.then(|| SyncIdleEmitter(app.clone()));
    // All triggers share this lock, preventing concurrent hashing, checks, and uploads.
    let _sync_guard = match coordinator.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if !is_auto {
                log_to_ui(
                    &app,
                    "INFO",
                    "Sync queued, waiting for running synchronization.",
                );
            }
            coordinator.lock().await
        }
    };
    let _ = app.emit("sync-started", ());
    audit_event(
        &app,
        audit_context.event(
            "sync.started",
            audit::Outcome::Started,
            audit::Severity::Info,
            "Synchronization started",
            json!({ "automatic": is_auto }),
        ),
    );
    let ScanResult {
        files, failures, ..
    } = scan_result;
    let total_files = files.len();
    let total_bytes: u64 = files.iter().map(|asset| asset.size).sum();
    let completed_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let success_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let scan_failure_count = failures.len();
    let failure_count =
        std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(scan_failure_count));
    for failure in failures {
        log_to_ui(
            &app,
            "ERROR",
            &format!("{}: {}", failure.path, failure.error),
        );
        if let Err(error) = db::mark_sync_failed(&pool, &failure.path, 0, 0).await {
            log_to_ui(
                &app,
                "ERROR",
                &format!("Could not persist failure for {}: {}", failure.path, error),
            );
        }
    }

    let prefix = if is_auto { "Auto-sync" } else { "Sync" };
    log_to_ui(
        &app,
        "INFO",
        &format!(
            "{}: Starting pipeline for {} files ({:.2} MB)",
            prefix,
            total_files,
            total_bytes as f64 / 1024.0 / 1024.0
        ),
    );

    // Pipeline Stage 1: Parallel Hashing (Concurrency = 4)
    let hashed_stream = futures::stream::iter(files)
        .map(|asset| {
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let failure_count = failure_count.clone();
            let audit_context = audit_context.clone();
            async move {
                let Asset { path, size, mtime } = asset;

                // Attempt cache hit only when we have a reliable mtime.
                let cached = match mtime {
                    0 => None,
                    _ => match db::get_cached_hash(&pool, &path, mtime, size as i64).await {
                        Ok(cached) => cached,
                        Err(error) => {
                            log_to_ui(
                                &app,
                                "ERROR",
                                &format!("Could not load cached hash for {}: {}", path, error),
                            );
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            completed_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                            audit_event(
                                &app,
                                audit_context.event(
                                    "db.cache_lookup.failed",
                                    audit::Outcome::Failure,
                                    audit::Severity::Error,
                                    "Cached checksum lookup failed",
                                    json!({
                                        "local_path": path,
                                        "failure_reason": audit_context.safe_error(&error.to_string()),
                                    }),
                                ),
                            );
                            return None;
                        }
                    },
                };

                // The persistent cache stores only Immich's SHA-1. Recompute the complete
                // checksum set so upload audit records always contain all three values.
                let (checksums, size, mtime) = {
                    let path_for_hash = path.clone();
                    match tokio::task::spawn_blocking(move || calculate_stable_checksums(&path_for_hash))
                        .await
                    {
                        Ok(Ok((checksums, size, mtime))) => {
                            let hash = &checksums.sha1_base64;
                            if cached.as_deref() == Some(hash) {
                                return Some(sync::SyncAsset { path, size, checksums: Some(checksums), mtime });
                            }
                            if let Err(error) = db::update_sync_state(
                                &pool,
                                &path,
                                hash,
                                mtime,
                                size as i64,
                                "PENDING",
                                None,
                            )
                            .await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!(
                                        "Could not persist pending sync state for {}: {}",
                                        path, error
                                    ),
                                );
                                failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                completed_bytes
                                    .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                                audit_event(
                                    &app,
                                    audit_context.event(
                                        "db.sync_state.failed",
                                        audit::Outcome::Failure,
                                        audit::Severity::Error,
                                        "Could not persist pending sync state",
                                        json!({
                                            "local_path": path,
                                            "failure_reason": audit_context.safe_error(&error.to_string()),
                                        }),
                                    ),
                                );
                                return None;
                            }
                            (checksums, size, mtime)
                        }
                        Ok(Err(error)) => {
                            log_to_ui(
                                &app,
                                "ERROR",
                                &format!("Hashing failed for {}: {}", path, error),
                            );
                            if let Err(error) =
                                db::mark_sync_failed(&pool, &path, mtime, size as i64).await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!("Could not persist failure for {}: {}", path, error),
                                );
                            }
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            completed_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                            audit_event(
                                &app,
                                audit_context.event(
                                    "file.hash.failed",
                                    audit::Outcome::Failure,
                                    audit::Severity::Error,
                                    "File hashing failed",
                                    json!({
                                        "local_path": path,
                                        "failure_reason": audit_context.safe_error(&error.to_string()),
                                    }),
                                ),
                            );
                            return None;
                        }
                        Err(error) => {
                            log_to_ui(
                                &app,
                                "ERROR",
                                &format!("Hashing task failed for {}: {}", path, error),
                            );
                            if let Err(error) =
                                db::mark_sync_failed(&pool, &path, mtime, size as i64).await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!("Could not persist failure for {}: {}", path, error),
                                );
                            }
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            completed_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                            audit_event(
                                &app,
                                audit_context.event(
                                    "file.hash.failed",
                                    audit::Outcome::Failure,
                                    audit::Severity::Error,
                                    "File hashing task failed",
                                    json!({
                                        "local_path": path,
                                        "failure_reason": audit_context.safe_error(&error.to_string()),
                                    }),
                                ),
                            );
                            return None;
                        }
                    }
                };
                Some(sync::SyncAsset {
                    path,
                    size,
                    checksums: Some(checksums),
                    mtime,
                })
            }
        })
        .buffer_unordered(4)
        .filter_map(|x| async { x });

    // Pipeline Stage 2: Buffered Bulk Check (Batch = 500)
    let checked_batches = hashed_stream
        .chunks(500)
        .map(|chunk| {
            let client = client.clone();
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let failure_count = failure_count.clone();
            let audit_context = audit_context.clone();
            async move {
                let hashes: Vec<String> = chunk
                    .iter()
                    .filter_map(|a| {
                        a.checksums
                            .as_ref()
                            .map(|checksums| checksums.sha1_base64.clone())
                    })
                    .collect();
                let existing_hashes =
                    check_assets_exist_with_backoff(&client, hashes, &app, &audit_context).await?;

                let mut to_upload = Vec::new();
                for asset in chunk {
                    if let Some(checksums) = &asset.checksums {
                        let hash = &checksums.sha1_base64;
                        if existing_hashes.contains(hash) {
                            if let Err(error) = db::update_sync_state(
                                &pool,
                                &asset.path,
                                hash,
                                asset.mtime,
                                asset.size as i64,
                                "SYNCED",
                                None,
                            )
                            .await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!(
                                        "Could not persist synced state for {}: {}",
                                        asset.path, error
                                    ),
                                );
                                failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            completed_bytes
                                .fetch_add(asset.size, std::sync::atomic::Ordering::SeqCst);
                        } else {
                            to_upload.push(asset);
                        }
                    }
                }
                Ok::<_, String>(to_upload)
            }
        })
        .buffered(1); // One bulk check at a time to keep it orderly

    // Pipeline Stage 3: Concurrent Upload (Concurrency = 3)
    futures::pin_mut!(checked_batches);
    while let Some(to_upload) = checked_batches.try_next().await? {
        futures::stream::iter(to_upload)
            .map(|asset| {
                let client = client.clone();
                let app = app.clone();
                let pool = pool.clone();
                let completed_bytes = completed_bytes.clone();
                let success_count = success_count.clone();
                let failure_count = failure_count.clone();
                let audit_context = audit_context.clone();
                async move {
                    // Stage 1 guarantees hash is Some; guard defensively
                    //         so a future code path can't cause a silent panic.
                    let checksums = match asset.checksums.as_ref() {
                        Some(checksums) => checksums.clone(),
                        None => return,
                    };
                    let hash = checksums.sha1_base64.clone();
                    let file_operation_id = audit::AuditEvent::operation_id();
                    let upload_started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
                    let details = upload_details(&asset, &checksums, &file_operation_id, &upload_started_at, None, None, None);
                    audit_event(
                        &app,
                        audit_context.event(
                            "file.upload.started",
                            audit::Outcome::Started,
                            audit::Severity::Info,
                            "Upload started",
                            details,
                        ),
                    );
                    let _ = app.emit("sync-progress", &asset.path);
                    match client.upload_asset(&asset.path, &hash).await {
                        Ok(remote_id) => {
                            if let Err(error) = db::update_sync_state(
                                &pool,
                                &asset.path,
                                &hash,
                                asset.mtime,
                                asset.size as i64,
                                "SYNCED",
                                Some(&remote_id),
                            )
                            .await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!(
                                        "Upload completed but could not persist synced state for {}: {}",
                                        asset.path, error
                                    ),
                                );
                                failure_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            } else {
                                success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            }
                            let details = upload_details(&asset, &checksums, &file_operation_id, &upload_started_at, Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)), Some(&remote_id), None);
                            audit_event(
                                &app,
                                audit_context.event(
                                    "file.upload.completed",
                                    audit::Outcome::Success,
                                    audit::Severity::Info,
                                    "Upload completed",
                                    details,
                                ),
                            );
                        }
                        Err(e) => {
                            log_to_ui(
                                &app,
                                "ERROR",
                                &format!("Upload failed for {}: {}", asset.path, e),
                            );
                            if let Err(error) = db::mark_sync_failed(
                                &pool,
                                &asset.path,
                                asset.mtime,
                                asset.size as i64,
                            )
                            .await
                            {
                                log_to_ui(
                                    &app,
                                    "ERROR",
                                    &format!(
                                        "Could not persist failure for {}: {}",
                                        asset.path, error
                                    ),
                                );
                            }
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let details = upload_details(&asset, &checksums, &file_operation_id, &upload_started_at, Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)), None, Some(audit_context.safe_error(&e)));
                            audit_event(
                                &app,
                                audit_context.event(
                                    "file.upload.completed",
                                    audit::Outcome::Failure,
                                    audit::Severity::Error,
                                    "Upload failed",
                                    details,
                                ),
                            );
                        }
                    }
                    let done = completed_bytes
                        .fetch_add(asset.size, std::sync::atomic::Ordering::SeqCst)
                        + asset.size;
                    let pct = ((done as f64 / total_bytes as f64 * 100.0) as u32).min(100);
                    let _ = app.emit("sync-progress-percent", pct);
                }
            })
            .buffer_unordered(3)
            .collect::<Vec<_>>()
            .await;
    }

    let uploaded = success_count.load(std::sync::atomic::Ordering::SeqCst);
    let failed = failure_count.load(std::sync::atomic::Ordering::SeqCst);
    let processed = total_files.saturating_sub(failed.saturating_sub(scan_failure_count));
    if !is_auto || uploaded > 0 || failed > 0 {
        let locale_state = app.state::<LocaleState>();
        let locale = locale_state
            .0
            .lock()
            .map(|l| l.clone())
            .unwrap_or_else(|_| "en".to_string());

        let translations = backend_translations(&locale);
        let title = translations
            .notification_complete_title
            .replacen("{}", prefix, 1);
        let body = translations
            .notification_complete_body
            .replace("{processed}", &processed.to_string())
            .replace("{uploaded}", &uploaded.to_string())
            .replace("{failed}", &failed.to_string());

        send_notification(&app, &title, &body, "INFO");
    }

    let summary = SyncSummary {
        processed,
        uploaded,
        failed,
    };
    audit_event(
        &app,
        audit_context.event(
            "sync.completed",
            if summary.failed == 0 {
                audit::Outcome::Success
            } else {
                audit::Outcome::Failure
            },
            if summary.failed == 0 {
                audit::Severity::Info
            } else {
                audit::Severity::Warn
            },
            "Synchronization completed",
            json!({
                "processed": summary.processed,
                "uploaded": summary.uploaded,
                "failed": summary.failed,
            }),
        ),
    );
    Ok(summary)
}

async fn check_assets_exist_with_backoff(
    client: &sync::ImmichClient,
    hashes: Vec<String>,
    app: &tauri::AppHandle,
    audit_context: &SyncAuditContext,
) -> Result<Vec<String>, String> {
    for attempt in 1..=BULK_CHECK_MAX_ATTEMPTS {
        match client.check_assets_exist(hashes.clone()).await {
            Ok(existing) => return Ok(existing),
            Err(error) if !error.is_retryable() => {
                let message = format!("Bulk check cannot be retried; aborting sync: {}", error);
                log_to_ui(app, "ERROR", &message);
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Failure,
                        audit::Severity::Error,
                        "Bulk check cannot be retried",
                        json!({
                            "failure_reason": audit_context.safe_error(&error.to_string()),
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) if attempt == BULK_CHECK_MAX_ATTEMPTS => {
                let message = format!(
                    "Bulk check failed after {} attempts; aborting sync: {}",
                    attempt, error
                );
                log_to_ui(app, "ERROR", &message);
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Failure,
                        audit::Severity::Error,
                        "Bulk check failed",
                        json!({
                            "failure_reason": audit_context.safe_error(&error.to_string()),
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) => {
                let delay = BULK_CHECK_INITIAL_BACKOFF * (1 << (attempt - 1));
                log_to_ui(
                    app,
                    "WARN",
                    &format!(
                        "Bulk check attempt {}/{} failed: {}. Retrying in {} seconds.",
                        attempt,
                        BULK_CHECK_MAX_ATTEMPTS,
                        error,
                        delay.as_secs()
                    ),
                );
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Info,
                        audit::Severity::Warn,
                        "Bulk check retry scheduled",
                        json!({
                            "failure_reason": audit_context.safe_error(&error.to_string()),
                            "attempt": attempt,
                        }),
                    ),
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    unreachable!("the final bulk-check attempt always returns")
}

async fn sync_scan_result_if_authenticated(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    scan_result: ScanResult,
    source: &'static str,
) {
    log_overlapping_folders(&app, &scan_result);
    if scan_result.files.is_empty() && scan_result.failures.is_empty() {
        return;
    }

    if let Ok(Some(credentials)) = auth::get_credentials() {
        let audit_context = SyncAuditContext::from_credentials(
            &credentials.server_url,
            &credentials.api_key,
            source,
        );
        if let Some(client) = create_authenticated_client(&app, credentials) {
            let coordinator = app.state::<SyncCoordinator>().0.clone();
            if let Err(error) = run_sync_pipeline(
                app.clone(),
                pool,
                client,
                scan_result,
                true,
                coordinator,
                audit_context,
            )
            .await
            {
                emit_sync_error(&app, &error);
            }
        }
    }
}

#[tauri::command]
async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<SyncSummary, String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    ensure_sync_has_folders(&folders)?;

    let command_operation_id = audit::AuditEvent::operation_id();
    // Attempt to set sync_state to true. If it was already true, return early.
    if sync_state
        .0
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log_to_ui(
            &app,
            "WARN",
            "Manual sync already in progress. Ignoring request.",
        );
        return Err("Sync already in progress".to_string());
    }

    log_to_ui(&app, "INFO", "Starting manual synchronization...");

    // Ensure we reset the state when we're done, even if we fail.
    let result = async {
        let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
        let audit_context = SyncAuditContext::from_credentials(
            &credentials.server_url,
            &credentials.api_key,
            "manual_sync",
        );
        let client =
            create_authenticated_client(&app, credentials).ok_or("Invalid server configuration")?;

        for folder in &folders {
            log_to_ui(&app, "INFO", &format!("Scanning folder: {}", folder.path));
        }
        let folder_paths = folders
            .into_iter()
            .map(|folder| std::path::PathBuf::from(folder.path))
            .collect();
        let scan_result = scan_folders_for_media(folder_paths).await?;
        log_overlapping_folders(&app, &scan_result);

        run_sync_pipeline(
            app.clone(),
            pool.inner().clone(),
            client,
            scan_result,
            false,
            sync_coordinator.0.clone(),
            audit_context,
        )
        .await
    }
    .await;

    sync_state.0.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        audit_event(
            &app,
            audit::AuditEvent::new(
                &command_operation_id,
                "sync.aborted",
                audit::Outcome::Failure,
                audit::Severity::Error,
                None,
                "manual_sync",
                "Manual synchronization aborted",
                json!({ "failure_reason": audit_safe_error(error, None, None) }),
            ),
        );
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let audit_logger = audit::AuditLogger::initialize(&handle)
                .map_err(|error| format!("Failed to initialize audit logger: {}", error))?;
            handle.manage(std::sync::Arc::new(audit_logger));
            const WATCH_EVENT_QUEUE_CAPACITY: usize = 4_096;
            let (tx, mut rx) = tokio::sync::mpsc::channel(WATCH_EVENT_QUEUE_CAPACITY);
            let rescan_requested = std::sync::Arc::new(AtomicBool::new(false));
            let rescan_notify = std::sync::Arc::new(tokio::sync::Notify::new());

            // Initialize Watcher
            let watcher = watcher::create_watcher(
                tx,
                rescan_requested.clone(),
                rescan_notify.clone(),
            )
                .map_err(|e| e.to_string())?;

            // If DB init fails completely (e.g. disk full), return Err from setup() so app exits cleanly.
            // When database corruption is detected, db::init automatically quarantines and repairs it.
            let db_init = tauri::async_runtime::block_on(db::init(&handle))
                .map_err(|e| format!("Failed to initialize database: {}", e))?;
            let pool = db_init.pool;

            let mut recovery_notice = None;
            if let db::DatabaseStatus::Repaired {
                ref backup_path,
                salvaged_folders,
                ref reason,
            } = db_init.status
            {
                let notice = format!(
                    "Database corruption detected ({}). Quarantined backup to '{}' and restored {} folder(s).",
                    reason,
                    backup_path.display(),
                    salvaged_folders
                );
                send_notification(
                    &handle,
                    "Lymic - Database Repaired",
                    &notice,
                    "WARN",
                );
                audit_event(
                    &handle,
                    audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "database.repaired",
                        audit::Outcome::Success,
                        audit::Severity::Warn,
                        None,
                        "startup",
                        "Database corruption detected and repaired",
                        json!({
                            "backup_path": backup_path.to_string_lossy(),
                            "salvaged_folders": salvaged_folders,
                            "reason": reason,
                        }),
                    ),
                );
                recovery_notice = Some(notice);
            }

            // Watch existing folders
            let mut watcher = watcher::WatcherState::new(watcher);
            let folders = tauri::async_runtime::block_on(db::get_folders(&pool))
                .map_err(|e| format!("Failed to load watched folders: {}", e))?;
            let has_folders = !folders.is_empty();
            for folder in folders {
                // surface deferred-watch state at startup.
                match watcher::watch_path(&mut watcher, &folder.path) {
                    Ok(watcher::WatchRegistration::PathUnavailable) => {
                        log_to_ui(&handle, "WARN", &format!("Folder '{}' offline at startup; watching deferred.", folder.path));
                        audit_event(
                            &handle,
                            audit::AuditEvent::new(
                                audit::AuditEvent::operation_id(),
                                "watcher.registration",
                                audit::Outcome::Skipped,
                                audit::Severity::Warn,
                                None,
                                "startup",
                                "Watched folder is unavailable",
                                json!({ "local_path": folder.path }),
                            ),
                        );
                    }
                    Err(e) => {
                        log_to_ui(&handle, "WARN", &format!("Could not watch '{}': {}", folder.path, e));
                        audit_event(
                            &handle,
                            audit::AuditEvent::new(
                                audit::AuditEvent::operation_id(),
                                "watcher.registration",
                                audit::Outcome::Failure,
                                audit::Severity::Warn,
                                None,
                                "startup",
                                "Watcher registration failed",
                                json!({
                                    "local_path": folder.path,
                                    "failure_reason": audit_safe_error(&e.to_string(), None, None),
                                }),
                            ),
                        );
                    }
                    Ok(_) => {
                        audit_event(
                            &handle,
                            audit::AuditEvent::new(
                                audit::AuditEvent::operation_id(),
                                "watcher.registration",
                                audit::Outcome::Success,
                                audit::Severity::Info,
                                None,
                                "startup",
                                "Watcher registered",
                                json!({ "local_path": folder.path }),
                            ),
                        );
                    }
                }
            }

            handle.manage(pool);
            // tokio::sync::Mutex so async commands use .lock().await
            handle.manage(tokio::sync::Mutex::new(watcher));
            handle.manage(SyncState(AtomicBool::new(false)));
            handle.manage(SyncCoordinator(std::sync::Arc::new(tokio::sync::Mutex::new(()))));
            handle.manage(LocaleState(std::sync::Mutex::new("en".to_string())));
            handle.manage(DatabaseRecoveryNotice(std::sync::Mutex::new(recovery_notice)));
            audit_event(
                &handle,
                audit::AuditEvent::new(
                    audit::AuditEvent::operation_id(),
                    "app.started",
                    audit::Outcome::Success,
                    audit::Severity::Info,
                    None,
                    "startup",
                    "Application started",
                    json!({}),
                ),
            );

            // Missing folders are persisted so removable and network storage can be
            // selected while offline. Retry them periodically once they become available.
            let handle_deferred_watches = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut retry_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
                retry_interval.tick().await;

                loop {
                    retry_interval.tick().await;

                    let pool = handle_deferred_watches.state::<sqlx::SqlitePool>().inner().clone();
                    let folders = match db::get_folders(&pool).await {
                        Ok(folders) => folders,
                        Err(error) => {
                            log_to_ui(
                                &handle_deferred_watches,
                                "ERROR",
                                &format!("Could not load deferred watched folders: {}", error),
                            );
                            continue;
                        }
                    };

                    let available_paths = match tokio::task::spawn_blocking(move || {
                        folders
                            .into_iter()
                            .map(|folder| {
                                let path = std::path::PathBuf::from(folder.path);
                                let is_available = path.is_dir();
                                (path, is_available)
                            })
                            .collect::<Vec<_>>()
                    })
                    .await
                    {
                        Ok(paths) => paths,
                        Err(error) => {
                            log_to_ui(
                                &handle_deferred_watches,
                                "ERROR",
                                &format!("Could not check deferred watched folders: {}", error),
                            );
                            continue;
                        }
                    };

                    let registered_paths = {
                        let watcher = handle_deferred_watches
                            .state::<tokio::sync::Mutex<watcher::WatcherState>>();
                        let mut watcher = watcher.lock().await;
                        available_paths
                            .into_iter()
                            .filter_map(|(path, is_available)| match watcher.reconcile_path(
                                &path,
                                is_available,
                            ) {
                                Ok(watcher::WatchRegistration::Registered) => Some(path),
                                Ok(_) => None,
                                Err(error) => {
                                    log_to_ui(
                                        &handle_deferred_watches,
                                        "WARN",
                                        &format!("Could not watch '{}': {}", path.display(), error),
                                    );
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    };

                    if registered_paths.is_empty() {
                        continue;
                    }

                    for path in &registered_paths {
                        log_to_ui(
                            &handle_deferred_watches,
                            "INFO",
                            &format!("Folder '{}' is available; watching enabled.", path.display()),
                        );
                        audit_event(
                            &handle_deferred_watches,
                            audit::AuditEvent::new(
                                audit::AuditEvent::operation_id(),
                                "watcher.reconciliation",
                                audit::Outcome::Success,
                                audit::Severity::Info,
                                None,
                                "deferred_folder",
                                "Watcher registration reconciled",
                                json!({ "local_path": path }),
                            ),
                        );
                    }

                    let scan_result = match scan_folders_for_media(registered_paths.clone()).await {
                        Ok(scan_result) => scan_result,
                        Err(error) => {
                            log_to_ui(&handle_deferred_watches, "ERROR", &error);
                            continue;
                        }
                    };

                    sync_scan_result_if_authenticated(
                        handle_deferred_watches.clone(),
                        pool,
                        scan_result,
                        "deferred_folder",
                    )
                    .await;
                }
            });

            // Auto-sync on startup
            let handle_sync = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Give the UI a small delay to ensure it's ready for events
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                let pool = handle_sync.state::<sqlx::SqlitePool>().inner().clone();
                let folders = match db::get_folders(&pool).await {
                    Ok(folders) => folders,
                    Err(error) => {
                        log_to_ui(
                            &handle_sync,
                            "ERROR",
                            &format!("Could not load watched folders for startup sync: {}", error),
                        );
                        return;
                    }
                };
                for folder in &folders {
                    log_to_ui(
                        &handle_sync,
                        "INFO",
                        &format!("Scanning folder: {}", folder.path),
                    );
                }
                let folder_paths = folders
                    .into_iter()
                    .map(|folder| std::path::PathBuf::from(folder.path))
                    .collect();
                let scan_result = match scan_folders_for_media(folder_paths).await {
                    Ok(result) => result,
                    Err(error) => {
                        log_to_ui(&handle_sync, "ERROR", &error);
                        return;
                    }
                };

                sync_scan_result_if_authenticated(handle_sync.clone(), pool, scan_result, "startup_sync").await;
            });

            // Background Task to handle Watcher Events (Batched + debounced)
            let handle_task = handle.clone();
            let rescan_requested_task = rescan_requested.clone();
            let rescan_notify_task = rescan_notify.clone();
            tauri::async_runtime::spawn(async move {
                let mut files_buffer = std::collections::HashMap::new();
                let mut failures_buffer = std::collections::HashMap::new();
                loop {
                    tokio::select! {
                        Some(event) = rx.recv() => {
                            if let Err(e) = add_event_paths(&mut files_buffer, &mut failures_buffer, event).await {
                                log_to_ui(&handle_task, "ERROR", &e);
                            }
                        }
                        _ = rescan_notify_task.notified() => {}
                    }

                    // Debounce: wait for 2s of silence before processing
                    let debounce = tokio::time::sleep(tokio::time::Duration::from_secs(2));
                    tokio::pin!(debounce);
                    loop {
                        tokio::select! {
                            Some(event) = rx.recv() => {
                                if let Err(e) = add_event_paths(&mut files_buffer, &mut failures_buffer, event).await {
                                    log_to_ui(&handle_task, "ERROR", &e);
                                }
                                debounce.as_mut().reset(
                                    tokio::time::Instant::now()
                                        + tokio::time::Duration::from_secs(2),
                                );
                            }
                            _ = rescan_notify_task.notified() => {}
                            _ = &mut debounce => break,
                        }
                    }

                    if rescan_requested_task.swap(false, Ordering::AcqRel) {
                        log_to_ui(
                            &handle_task,
                            "WARN",
                            "File watcher event queue overflowed; reconciling all watched folders.",
                        );
                        audit_event(
                            &handle_task,
                            audit::AuditEvent::new(
                                audit::AuditEvent::operation_id(),
                                "watcher.rescan",
                                audit::Outcome::Started,
                                audit::Severity::Warn,
                                None,
                                "watcher",
                                "Watcher queue overflow triggered rescan",
                                json!({}),
                            ),
                        );
                        // The full folder scan is a superset of buffered paths, so discard
                        // them to avoid duplicate work after an event-stream loss.
                        files_buffer.clear();
                        failures_buffer.clear();
                        let pool = handle_task.state::<sqlx::SqlitePool>();
                        match db::get_folders(pool.inner()).await {
                            Ok(folders) => {
                                let folder_paths = folders
                                    .into_iter()
                                    .map(|folder| std::path::PathBuf::from(folder.path))
                                    .collect();
                                match scan_folders_for_media(folder_paths).await {
                                    Ok(scan_result) => {
                                        log_overlapping_folders(&handle_task, &scan_result);
                                        for asset in scan_result.files {
                                            files_buffer.insert(asset.path.clone(), asset);
                                        }
                                        for failure in scan_result.failures {
                                            failures_buffer.insert(failure.path.clone(), failure);
                                        }
                                    }
                                    Err(e) => log_to_ui(&handle_task, "ERROR", &e),
                                }
                            }
                            Err(e) => log_to_ui(
                                &handle_task,
                                "ERROR",
                                &format!("Could not reconcile watched folders after event loss: {}", e),
                            ),
                        }
                    }

                    if !files_buffer.is_empty() || !failures_buffer.is_empty() {
                        let mut scan_result = ScanResult::default();
                        scan_result.files.extend(files_buffer.drain().map(|(_, asset)| asset));
                        scan_result.failures.extend(failures_buffer.drain().map(|(_, failure)| failure));

                        let pool = handle_task.state::<sqlx::SqlitePool>();
                        let pool_inner = pool.inner().clone();

                        if let Ok(Some(creds)) = auth::get_credentials() {
                            let audit_context = SyncAuditContext::from_credentials(
                                &creds.server_url,
                                &creds.api_key,
                                "watcher_sync",
                            );
                            if let Some(client) = create_authenticated_client(&handle_task, creds) {
                                let coordinator = handle_task.state::<SyncCoordinator>().0.clone();
                                if let Err(error) = run_sync_pipeline(
                                    handle_task.clone(),
                                    pool_inner,
                                    client,
                                    scan_result,
                                    true,
                                    coordinator,
                                    audit_context,
                                )
                                .await
                                {
                                    emit_sync_error(&handle_task, &error);
                                }
                            }
                        }
                    }
                }
            });

            // Setup Tray Icon
            // greet() command removed (scaffold dead code)
            let translations = backend_translations("en");
            let quit_i = MenuItem::with_id(app, "quit", &translations.tray_quit, true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", &translations.tray_show, true, None::<&str>)?;
            let sync_i = MenuItem::with_id(app, "sync", &translations.tray_sync, has_folders, None::<&str>)?;
            let menu_items: &[&dyn tauri::menu::IsMenuItem<tauri::Wry>] = &[
                &sync_i,
                &show_i,
                &quit_i,
            ];
            let menu = Menu::with_items(app, menu_items)?;
            handle.manage(TrayMenuState(menu.clone()));

            let icon = app
                .default_window_icon()
                .ok_or("No default window icon configured in tauri.conf.json")?
                .clone();

            let tray_builder = TrayIconBuilder::with_id("main")
                .icon(icon)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
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
                        // On macOS, the menu usually shows on left click.
                        // On Windows, we often want to show the window on left click.
                        #[cfg(not(target_os = "macos"))]
                        {
                            let app = _tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                });

            // On macOS, it's standard to show the menu on left click.
            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.show_menu_on_left_click(true);

            #[cfg(not(target_os = "macos"))]
            let tray_builder = tray_builder.show_menu_on_left_click(false);

            let _tray = tray_builder.build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // greet removed
            login,
            logout,
            get_auth_status,
            get_server_url,
            get_current_user_name,
            add_folder,
            get_folders,
            remove_folder,
            start_sync,
            update_locale,
            get_database_recovery_notice
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(logger) = app
                    .try_state::<std::sync::Arc<audit::AuditLogger>>()
                    .map(|state| state.inner().clone())
                {
                    let _ = logger.append(audit::AuditEvent::new(
                        audit::AuditEvent::operation_id(),
                        "app.shutdown",
                        audit::Outcome::Success,
                        audit::Severity::Info,
                        None,
                        "shutdown",
                        "Application stopped",
                        json!({}),
                    ));
                    let _ = logger.flush();
                }
            }
        });
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    #[test]
    fn validates_all_supported_locales_and_translations() {
        assert!(is_supported_locale("en"));
        assert!(is_supported_locale("de"));
        assert!(!is_supported_locale("fr"));
        assert!(!is_supported_locale(""));

        let en = backend_translations("en");
        assert!(!en.tray_quit.is_empty());
        assert!(!en.notification_complete_body.is_empty());

        let de = backend_translations("de");
        assert!(!de.tray_quit.is_empty());
        assert!(!de.notification_complete_body.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "lymic-scan-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn sync_folder_guard_rejects_an_empty_database() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let folders = db::get_folders(&pool).await.unwrap();
        assert_eq!(
            ensure_sync_has_folders(&folders),
            Err("Add at least one folder before starting a sync.".to_string())
        );

        db::add_folder(&pool, "C:/photos").await.unwrap();
        let folders = db::get_folders(&pool).await.unwrap();
        assert!(ensure_sync_has_folders(&folders).is_ok());
    }

    #[tokio::test]
    async fn overlapping_folders_scan_each_media_file_once() {
        let root = temporary_directory();
        let child = root.join("Vacation");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("photo.jpg"), b"test image").unwrap();

        let result = scan_folders_for_media(vec![root.clone(), child])
            .await
            .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.overlapping_folders.len(), 1);
        assert!(!result.files[0].path.starts_with(r"\\?\"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognizes_all_immich_supported_asset_extensions() {
        for extension in MEDIA_EXTENSIONS {
            assert!(is_media_file(std::path::Path::new(&format!(
                "asset.{extension}"
            ))));
            assert!(is_media_file(std::path::Path::new(&format!(
                "asset.{}",
                extension.to_uppercase()
            ))));
        }

        assert!(!is_media_file(std::path::Path::new("asset.xmp")));
        assert!(!is_media_file(std::path::Path::new("asset.txt")));
    }

    #[test]
    fn stable_checksums_use_the_hashed_file_version() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"test image").unwrap();

        let path_string = path.to_string_lossy().to_string();
        let (checksums, hashed_size, hashed_mtime) =
            calculate_stable_checksums(&path_string).unwrap();

        assert_eq!(checksums, sync::calculate_checksums(&path_string).unwrap());
        assert_eq!(
            (hashed_size, hashed_mtime),
            file_version(&path_string).unwrap()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stable_checksums_use_current_version_when_file_changes_after_scan() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"old").unwrap();

        let path_string = path.to_string_lossy().to_string();
        let (old_size, _) = file_version(&path_string).unwrap();
        std::fs::write(&path, b"updated image content").unwrap();

        let (checksums, size, mtime) = calculate_stable_checksums(&path_string).unwrap();

        assert_eq!(checksums, sync::calculate_checksums(&path_string).unwrap());
        assert_eq!((size, mtime), file_version(&path_string).unwrap());
        assert_ne!(size, old_size);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upload_audit_details_include_all_required_checksums() {
        let asset = sync::SyncAsset {
            path: "C:/media/photo.jpg".to_string(),
            size: 123,
            checksums: None,
            mtime: 0,
        };
        let checksums = sync::Checksums {
            md5_hex: "a".repeat(32),
            sha256_hex: "b".repeat(64),
            sha1_base64: "c2hhMQ==".to_string(),
        };
        let details = upload_details(
            &asset,
            &checksums,
            "file-operation",
            "2026-10-04T00:00:00.000000000Z",
            Some("2026-10-04T00:00:01.000000000Z".to_string()),
            Some("remote-id"),
            None,
        );
        assert_eq!(details["file_size_bytes"], 123);
        assert_eq!(details["checksums"]["md5"], checksums.md5_hex);
        assert_eq!(details["checksums"]["sha256"], checksums.sha256_hex);
        assert_eq!(details["checksums"]["sha1_base64"], checksums.sha1_base64);
        assert_eq!(details["remote_asset_id"], "remote-id");
        assert!(details.get("failure_reason").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn removes_verbatim_prefix_and_compares_paths_case_insensitively() {
        let parent = remove_windows_verbatim_prefix(PathBuf::from(r"\\?\C:\Fotos"));
        let child = PathBuf::from(r"c:\fotos\Urlaub");

        assert_eq!(parent, PathBuf::from(r"C:\Fotos"));
        assert!(path_is_within(&child, &parent));
        assert!(path_is_within(&parent, &PathBuf::from(r"c:\FOTOS")));
    }
}
