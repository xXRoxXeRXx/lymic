mod auth;
mod db;
mod sync;
mod watcher;

use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri::{Emitter, Manager};
use futures::StreamExt;
use tauri_plugin_notification::NotificationExt;
use std::sync::atomic::{AtomicBool, Ordering};

struct SyncState(AtomicBool);
struct SyncCoordinator(std::sync::Arc<tokio::sync::Mutex<()>>);
struct LocaleState(std::sync::Mutex<String>);
struct TrayMenuState(Menu<tauri::Wry>);

// ---------------------------------------------------------------------------
// Supported media file extensions for sync (mirrors start_sync scan filter).
// Defined once and reused in both start_sync and the watcher path.
// ---------------------------------------------------------------------------
// removed redundant `as &[&str]` cast.
const MEDIA_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "heic", "heif", "mp4", "mov", "avi"];

fn is_media_file(path: &std::path::Path) -> bool {
    // eq_ignore_ascii_case avoids a heap allocation per file.
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEDIA_EXTENSIONS.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
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
}

fn metadata_mtime(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .or_else(|_| metadata.created())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
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
                path: error
                    .path()
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string(),
                error: format!("Could not scan path: {}", error),
            }),
        }
    }
    result
}

async fn scan_folders_for_media(
    paths: Vec<std::path::PathBuf>,
) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            let scan_result = scan_folder_for_media(&path);
            result.files.extend(scan_result.files);
            result.failures.extend(scan_result.failures);
        }
        result
    })
    .await
    .map_err(|e| format!("Media scan task failed: {}", e))
}

async fn scan_paths_for_media(
    paths: Vec<std::path::PathBuf>,
) -> Result<ScanResult, String> {
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
    let mut directories = Vec::new();
    let mut files = Vec::new();
    for path in event.paths {
        if path.is_dir() {
            directories.push(path);
        } else if is_media_file(&path) {
            files.push(path);
        }
    }

    for scan_result in [scan_folders_for_media(directories).await?, scan_paths_for_media(files).await?] {
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
    let _ = app.notification()
        .builder()
        .title(title)
        .body(body)
        .show();
    log_to_ui(app, level, body);
}

fn log_to_ui(app: &tauri::AppHandle, level: &str, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_line = format!("[{}] [{}] {}", timestamp, level, message);
    let _ = app.emit("log-message", log_line);
}

// Login reuses ImmichClient's pooled reqwest::Client instead of
//         creating a one-off Client::new() that bypasses connection pooling.
// (security): api_key is intentionally kept out of all log/error
//         messages below. reqwest error Display does not include headers,
//         so the key is not leaked through map_err strings either.
#[tauri::command]
async fn login(app: tauri::AppHandle, server_url: String, api_key: String) -> Result<(), String> {
    log_to_ui(&app, "INFO", &format!("Attempting to connect to {}", server_url));

    // Build a temporary ImmichClient purely to reuse its URL normalisation and
    // pooled reqwest Client. The client is discarded after the connection test.
    let temp_client = sync::ImmichClient::new(server_url.clone(), api_key.clone());
    let url = format!("{}/server/config", temp_client.base_url());

    let response = temp_client
        .http_client()
        .get(&url)
        .header("x-api-key", &api_key)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Server returned error: {}", response.status()));
    }

    auth::store_credentials(&server_url, &api_key)
}

#[tauri::command]
async fn logout() -> Result<(), String> {
    auth::delete_credentials()
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
fn update_locale(app: tauri::AppHandle, state: tauri::State<'_, LocaleState>, locale: String) -> Result<(), String> {
    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    *current = locale.clone();
    
    // Update tray menu labels
    let quit_label = if locale == "de" { "Beenden" } else { "Quit" };
    let show_label = if locale == "de" { "Fenster anzeigen" } else { "Show Window" };
    let sync_label = if locale == "de" { "Jetzt synchronisieren" } else { "Sync Now" };

    let tray_menu = app.state::<TrayMenuState>();
    let menu = &tray_menu.0;
    
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("quit") {
        let _ = item.set_text(quit_label);
    }
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("show") {
        let _ = item.set_text(show_label);
    }
    if let Some(MenuItemKind::MenuItem(item)) = menu.get("sync") {
        let _ = item.set_text(sync_label);
    }
    
    Ok(())
}

// tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<notify::RecommendedWatcher>>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
    path: String,
) -> Result<i64, String> {
    let id = db::add_folder(pool.inner(), &path)
        .await
        .map_err(|e| e.to_string())?;
    
    let mut w = watcher.lock().await;
    // warn when the path doesn't exist (e.g. offline drive).
    match watcher::watch_path(&mut w, &path) {
        Ok(false) => log_to_ui(&app, "WARN", &format!("Folder '{}' does not exist; watching deferred until it appears.", path)),
        Err(e)   => log_to_ui(&app, "WARN", &format!("Could not watch '{}': {}", path, e)),
        Ok(true) => {}
    }

    // Auto-sync the new folder immediately in background
    let pool_inner = pool.inner().clone();
    let coordinator = sync_coordinator.0.clone();
    let path_clone = path.clone();
    tauri::async_runtime::spawn(async move {
        let scan_result = match scan_folders_for_media(vec![std::path::PathBuf::from(path_clone)]).await {
            Ok(result) => result,
            Err(error) => {
                log_to_ui(&app, "ERROR", &error);
                return;
            }
        };
        if let Ok(Some(creds)) = auth::get_credentials() {
            let client = std::sync::Arc::new(sync::ImmichClient::new(creds.server_url, creds.api_key));
            let _ = run_sync_pipeline(app, pool_inner, client, scan_result, true, coordinator).await;
        }
    });

    Ok(id)
}

#[tauri::command]
async fn get_folders(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::WatchedFolder>, String> {
    db::get_folders(pool.inner()).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<notify::RecommendedWatcher>>,
    id: i64,
) -> Result<(), String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    // delete the DB row first so concurrent readers no longer see
    //         this folder; then unwatch. This closes the race window where a
    //         filesystem event could trigger a sync on a just-removed folder.
    db::remove_folder(pool.inner(), id)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let mut w = watcher.lock().await;
        // log unwatch failures instead of silently discarding them.
        if let Err(e) = watcher::unwatch_path(&mut w, &folder.path) {
            log_to_ui(&app, "WARN", &format!("Failed to unwatch '{}': {}", folder.path, e));
        }
    }
    Ok(())
}

async fn run_sync_pipeline(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: std::sync::Arc<sync::ImmichClient>,
    scan_result: ScanResult,
    is_auto: bool,
    coordinator: std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
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
    let ScanResult { files, failures } = scan_result;
    let total_files = files.len();
    let total_bytes: u64 = files.iter().map(|asset| asset.size).sum();
    let completed_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let success_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let scan_failure_count = failures.len();
    let failure_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(scan_failure_count));
    let device_id = format!("{}-IMMICH-DESKTOP", std::env::consts::OS.to_uppercase());

    for failure in failures {
        log_to_ui(
            &app,
            "ERROR",
            &format!("{}: {}", failure.path, failure.error),
        );
        if let Err(error) = db::mark_sync_failed(&pool, &failure.path, 0, 0).await {
            log_to_ui(&app, "ERROR", &format!("Could not persist failure for {}: {}", failure.path, error));
        }
    }

    let prefix = if is_auto { "Auto-sync" } else { "Sync" };
    log_to_ui(&app, "INFO", &format!("{}: Starting pipeline for {} files ({:.2} MB)", 
        prefix, total_files, total_bytes as f64 / 1024.0 / 1024.0));

    // Pipeline Stage 1: Parallel Hashing (Concurrency = 4)
    let hashed_stream = futures::stream::iter(files)
        .map(|asset| {
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            let failure_count = failure_count.clone();
            async move {
                let Asset { path, size, mtime } = asset;

                // Attempt cache hit only when we have a reliable mtime.
                let cached = match mtime {
                    0 => None,
                    _ => db::get_cached_hash(&pool, &path, mtime, size as i64).await,
                };

                let hash = if let Some(h) = cached {
                    h
                } else {
                    // SHA-1 is blocking CPU+I/O — run it off the async executor.
                    let path_for_hash = path.clone();
                    match tokio::task::spawn_blocking(move || sync::calculate_hash(&path_for_hash)).await {
                        Ok(Ok(h)) => {
                            let _ = db::update_sync_state(&pool, &path, &h, mtime, size as i64, "PENDING", None).await;
                            h
                        }
                        Ok(Err(error)) => {
                            log_to_ui(&app, "ERROR", &format!("Hashing failed for {}: {}", path, error));
                            if let Err(error) = db::mark_sync_failed(&pool, &path, mtime, size as i64).await {
                                log_to_ui(&app, "ERROR", &format!("Could not persist failure for {}: {}", path, error));
                            }
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            completed_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                            return None;
                        }
                        Err(error) => {
                            log_to_ui(&app, "ERROR", &format!("Hashing task failed for {}: {}", path, error));
                            if let Err(error) = db::mark_sync_failed(&pool, &path, mtime, size as i64).await {
                                log_to_ui(&app, "ERROR", &format!("Could not persist failure for {}: {}", path, error));
                            }
                            failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            completed_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                            return None;
                        }
                    }
                };
                Some(sync::SyncAsset { path, size, hash: Some(hash), mtime })
            }
        })
        .buffer_unordered(4)
        .filter_map(|x| async { x });

    // Pipeline Stage 2: Buffered Bulk Check (Batch = 500)
    let checked_stream = hashed_stream
        .chunks(500)
        .map(|chunk| {
            let client = client.clone();
            let pool = pool.clone();
            let app = app.clone();
            let completed_bytes = completed_bytes.clone();
            async move {
                let hashes: Vec<String> = chunk.iter().filter_map(|a| a.hash.clone()).collect();
                let existing_hashes = match client.check_assets_exist(hashes).await {
                    Ok(existing) => existing,
                    Err(e) => {
                        log_to_ui(&app, "ERROR", &format!("Bulk check failed: {}", e));
                        Vec::new()
                    }
                };

                let mut to_upload = Vec::new();
                for asset in chunk {
                    if let Some(hash) = &asset.hash {
                        if existing_hashes.contains(hash) {
                            let _ = db::update_sync_state(&pool, &asset.path, hash, asset.mtime, asset.size as i64, "SYNCED", None).await;
                            completed_bytes.fetch_add(asset.size, std::sync::atomic::Ordering::SeqCst);
                        } else {
                            to_upload.push(asset);
                        }
                    }
                }
                to_upload
            }
        })
        .buffered(1) // One bulk check at a time to keep it orderly
        .flat_map(futures::stream::iter);

    // Pipeline Stage 3: Concurrent Upload (Concurrency = 3)
    checked_stream
        .map(|asset| {
            let client = client.clone();
            let device_id = device_id.clone();
            let app = app.clone();
            let pool = pool.clone();
            let completed_bytes = completed_bytes.clone();
            let success_count = success_count.clone();
            let failure_count = failure_count.clone();
            async move {
                // Stage 1 guarantees hash is Some; guard defensively
                //         so a future code path can't cause a silent panic.
                let hash = match asset.hash.as_ref() {
                    Some(h) => h.clone(),
                    None => return,
                };
                let _ = app.emit("sync-progress", &asset.path);
                match client.upload_asset(&asset.path, &device_id, &hash).await {
                    Ok(remote_id) => {
                        let _ = db::update_sync_state(&pool, &asset.path, &hash, asset.mtime, asset.size as i64, "SYNCED", Some(&remote_id)).await;
                        success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(e) => {
                        log_to_ui(&app, "ERROR", &format!("Upload failed for {}: {}", asset.path, e));
                        if let Err(error) = db::mark_sync_failed(&pool, &asset.path, asset.mtime, asset.size as i64).await {
                            log_to_ui(&app, "ERROR", &format!("Could not persist failure for {}: {}", asset.path, error));
                        }
                        failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                let done = completed_bytes.fetch_add(asset.size, std::sync::atomic::Ordering::SeqCst) + asset.size;
                let pct = ((done as f64 / total_bytes as f64 * 100.0) as u32).min(100);
                let _ = app.emit("sync-progress-percent", pct);
            }
        })
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await;

    let uploaded = success_count.load(std::sync::atomic::Ordering::SeqCst);
    let failed = failure_count.load(std::sync::atomic::Ordering::SeqCst);
    let processed = total_files.saturating_sub(failed.saturating_sub(scan_failure_count));
    if !is_auto || uploaded > 0 || failed > 0 {
        let locale_state = app.state::<LocaleState>();
        let locale = locale_state.0.lock().map(|l| l.clone()).unwrap_or_else(|_| "en".to_string());
        
        let title = if locale == "de" {
            format!("{} Abgeschlossen", prefix)
        } else {
            format!("{} Complete", prefix)
        };
        
        let body = if locale == "de" {
            format!("{} Dateien verarbeitet. {} neue Uploads. {} Fehler.", processed, uploaded, failed)
        } else {
            format!("Processed {} files. {} new uploads. {} errors.", processed, uploaded, failed)
        };

        send_notification(
            &app,
            &title,
            &body,
            "INFO",
        );
    }
    
    let _ = app.emit("sync-idle", ());
    Ok(())
}

#[tauri::command]
async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    sync_state: tauri::State<'_, SyncState>,
    sync_coordinator: tauri::State<'_, SyncCoordinator>,
) -> Result<(), String> {
    // Attempt to set sync_state to true. If it was already true, return early.
    if sync_state.0.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        log_to_ui(&app, "WARN", "Manual sync already in progress. Ignoring request.");
        return Ok(());
    }

    log_to_ui(&app, "INFO", "Starting manual synchronization...");
    
    // Ensure we reset the state when we're done, even if we fail.
    let result = async {
        let folders = db::get_folders(pool.inner()).await.map_err(|e| e.to_string())?;
        let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
        let client = std::sync::Arc::new(sync::ImmichClient::new(credentials.server_url, credentials.api_key));

        for folder in &folders {
            log_to_ui(&app, "INFO", &format!("Scanning folder: {}", folder.path));
        }
        let folder_paths = folders
            .into_iter()
            .map(|folder| std::path::PathBuf::from(folder.path))
            .collect();
        let scan_result = scan_folders_for_media(folder_paths).await?;

        run_sync_pipeline(
            app,
            pool.inner().clone(),
            client,
            scan_result,
            false,
            sync_coordinator.0.clone(),
        ).await
    }.await;

    sync_state.0.store(false, Ordering::SeqCst);
    result
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
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

            // if DB init fails, return Err from setup() so the app exits
            //         cleanly instead of silently continuing without managed state.
            let pool = tauri::async_runtime::block_on(db::init(&handle))
                .map_err(|e| format!("Failed to initialize database: {}", e))?;

            // Watch existing folders
            let mut watcher = watcher;
            if let Ok(folders) = tauri::async_runtime::block_on(db::get_folders(&pool)) {
                for folder in folders {
                    // surface deferred-watch state at startup.
                    match watcher::watch_path(&mut watcher, &folder.path) {
                        Ok(false) => eprintln!("[WARN] Folder '{}' offline at startup; watching deferred.", folder.path),
                        Err(e)   => eprintln!("[WARN] Could not watch '{}': {}", folder.path, e),
                        Ok(true) => {}
                    }
                }
            }

            handle.manage(pool);
            // tokio::sync::Mutex so async commands use .lock().await
            handle.manage(tokio::sync::Mutex::new(watcher));
            handle.manage(SyncState(AtomicBool::new(false)));
            handle.manage(SyncCoordinator(std::sync::Arc::new(tokio::sync::Mutex::new(()))));
            handle.manage(LocaleState(std::sync::Mutex::new("de".to_string())));

            // Auto-sync on startup
            let handle_sync = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Give the UI a small delay to ensure it's ready for events
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                
                if let Ok(Some(creds)) = auth::get_credentials() {
                    let pool = handle_sync.state::<sqlx::SqlitePool>().inner().clone();
                    if let Ok(folders) = db::get_folders(&pool).await {
                        for folder in &folders {
                            log_to_ui(&handle_sync, "INFO", &format!("Scanning folder: {}", folder.path));
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
                        
                        if !scan_result.files.is_empty() || !scan_result.failures.is_empty() {
                            let client = std::sync::Arc::new(sync::ImmichClient::new(creds.server_url, creds.api_key));
                            let coordinator = handle_sync.state::<SyncCoordinator>().0.clone();
                            let _ = run_sync_pipeline(handle_sync, pool, client, scan_result, true, coordinator).await;
                        }
                    }
                }
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
                            let client = std::sync::Arc::new(sync::ImmichClient::new(
                                creds.server_url,
                                creds.api_key,
                            ));
                            let coordinator = handle_task.state::<SyncCoordinator>().0.clone();
                            let _ = run_sync_pipeline(
                                handle_task.clone(),
                                pool_inner,
                                client,
                                scan_result,
                                true,
                                coordinator,
                            ).await;
                        }
                    }
                }
            });

            // Setup Tray Icon
            // greet() command removed (scaffold dead code)
            let quit_i = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Fenster anzeigen", true, None::<&str>)?;
            let sync_i = MenuItem::with_id(app, "sync", "Jetzt synchronisieren", true, None::<&str>)?;
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
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        
                        // On macOS, the menu usually shows on left click.
                        // On Windows, we often want to show the window on left click.
                        #[cfg(not(target_os = "macos"))]
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
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
            add_folder,
            get_folders,
            remove_folder,
            start_sync,
            update_locale
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
