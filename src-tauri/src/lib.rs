mod auth;
mod db;
mod sync;
mod watcher;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri::{Emitter, Manager};
use futures::StreamExt;
use tauri_plugin_notification::NotificationExt;

// ---------------------------------------------------------------------------
// Supported media file extensions for sync (mirrors start_sync scan filter).
// Fix 9: defined once and reused in both start_sync and the watcher path.
// ---------------------------------------------------------------------------
const MEDIA_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "heic", "heif", "mp4", "mov", "avi"] as &[&str];

fn is_media_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEDIA_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn scan_folder_for_media(path: &std::path::Path) -> Vec<(String, u64)> {
    let mut files = Vec::new();
    if path.exists() {
        let walker = walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok());
        for entry in walker {
            if entry.file_type().is_file() && is_media_file(entry.path()) {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((entry.path().to_string_lossy().to_string(), size));
            }
        }
    }
    files
}

fn send_notification(app: &tauri::AppHandle, title: &str, body: &str) {
    let _ = app.notification()
        .builder()
        .title(title)
        .body(body)
        .show();
    log_to_ui(app, "INFO", body);
}

fn log_to_ui(app: &tauri::AppHandle, level: &str, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let log_line = format!("[{}] [{}] {}", timestamp, level, message);
    let _ = app.emit("log-message", log_line);
}

// Fix 15: Login reuses ImmichClient's pooled reqwest::Client instead of
//         creating a one-off Client::new() that bypasses connection pooling.
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

// Fix 6: tokio::sync::Mutex avoids blocking the async executor while waiting
//         for the lock (std::sync::Mutex::lock blocks the current thread).
#[tauri::command]
async fn add_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<notify::RecommendedWatcher>>,
    path: String,
) -> Result<i64, String> {
    let id = db::add_folder(pool.inner(), &path)
        .await
        .map_err(|e| e.to_string())?;
    
    let mut w = watcher.lock().await;
    let _ = watcher::watch_path(&mut w, &path);

    // Auto-sync the new folder immediately in background
    let pool_inner = pool.inner().clone();
    let path_clone = path.clone();
    tauri::async_runtime::spawn(async move {
        let files = scan_folder_for_media(std::path::Path::new(&path_clone));
        if let Ok(Some(creds)) = auth::get_credentials() {
            let client = std::sync::Arc::new(sync::ImmichClient::new(creds.server_url, creds.api_key));
            let _ = run_sync_pipeline(app, pool_inner, client, files, true).await;
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
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, tokio::sync::Mutex<notify::RecommendedWatcher>>,
    id: i64,
) -> Result<(), String> {
    let folders = db::get_folders(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        // Fix 2+6: tokio Mutex with .await, no unwrap()
        let mut w = watcher.lock().await;
        let _ = watcher::unwatch_path(&mut w, &folder.path);
    }
    db::remove_folder(pool.inner(), id)
        .await
        .map_err(|e| e.to_string())
}

async fn run_sync_pipeline(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: std::sync::Arc<sync::ImmichClient>,
    files: Vec<(String, u64)>,
    is_auto: bool,
) -> Result<(), String> {
    let total_files = files.len();
    let total_bytes: u64 = files.iter().map(|(_, s)| *s).sum();
    let completed_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let success_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let device_id = format!("{}-IMMICH-DESKTOP", std::env::consts::OS.to_uppercase());

    if files.is_empty() {
        return Ok(());
    }

    let prefix = if is_auto { "Auto-sync" } else { "Sync" };
    log_to_ui(&app, "INFO", &format!("{}: Starting pipeline for {} files ({:.2} MB)", 
        prefix, total_files, total_bytes as f64 / 1024.0 / 1024.0));

    // Pipeline Stage 1: Parallel Hashing (Concurrency = 4)
    let hashed_stream = futures::stream::iter(files)
        .map(|(path, size)| {
            let pool = pool.clone();
            async move {
                let metadata = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => return None,
                };
                let mtime = metadata.modified().or_else(|_| metadata.created()).map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
                }).unwrap_or(0);
                
                let hash = if let Some(h) = db::get_cached_hash(&pool, &path, mtime, size as i64).await {
                    h
                } else {
                    match sync::calculate_hash(&path) {
                        Ok(h) => {
                            let _ = db::update_sync_state(&pool, &path, &h, mtime, size as i64, "PENDING", None).await;
                            h
                        }
                        Err(_) => return None,
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
            async move {
                let hash = asset.hash.as_ref().unwrap();
                let _ = app.emit("sync-progress", &asset.path);
                match client.upload_asset(&asset.path, &device_id, hash).await {
                    Ok(remote_id) => {
                        let _ = db::update_sync_state(&pool, &asset.path, hash, asset.mtime, asset.size as i64, "SYNCED", Some(&remote_id)).await;
                        success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(e) => {
                        log_to_ui(&app, "ERROR", &format!("Upload failed for {}: {}", asset.path, e));
                        let _ = db::update_sync_state(&pool, &asset.path, hash, asset.mtime, asset.size as i64, "FAILED", None).await;
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
    if !is_auto || uploaded > 0 {
        send_notification(&app, &format!("{} Complete", prefix), 
            &format!("Processed {} files. {} new uploads.", total_files, uploaded));
    }
    
    let _ = app.emit("sync-idle", ());
    Ok(())
}

#[tauri::command]
async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<(), String> {
    log_to_ui(&app, "INFO", "Starting manual synchronization...");
    let folders = db::get_folders(pool.inner()).await.map_err(|e| e.to_string())?;
    let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
    let client = std::sync::Arc::new(sync::ImmichClient::new(credentials.server_url, credentials.api_key));

    let mut all_files = Vec::new();
    for folder in folders {
        log_to_ui(&app, "INFO", &format!("Scanning folder: {}", folder.path));
        all_files.extend(scan_folder_for_media(std::path::Path::new(&folder.path)));
    }

    run_sync_pipeline(app, pool.inner().clone(), client, all_files, false).await
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
            let (tx, mut rx) = tokio::sync::mpsc::channel(100);

            // Initialize Watcher
            let watcher = watcher::create_watcher(tx).map_err(|e| e.to_string())?;

            // Fix 7: if DB init fails, return Err from setup() so the app exits
            //         cleanly instead of silently continuing without managed state.
            let pool = tauri::async_runtime::block_on(db::init(&handle))
                .map_err(|e| format!("Failed to initialize database: {}", e))?;

            // Watch existing folders
            let mut watcher = watcher;
            if let Ok(folders) = tauri::async_runtime::block_on(db::get_folders(&pool)) {
                for folder in folders {
                    let _ = watcher::watch_path(&mut watcher, &folder.path);
                }
            }

            handle.manage(pool);
            // Fix 6: tokio::sync::Mutex so async commands use .lock().await
            handle.manage(tokio::sync::Mutex::new(watcher));

            // Auto-sync on startup
            let handle_sync = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Give the UI a small delay to ensure it's ready for events
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                
                if let Ok(Some(creds)) = auth::get_credentials() {
                    let pool = handle_sync.state::<sqlx::SqlitePool>().inner().clone();
                    if let Ok(folders) = db::get_folders(&pool).await {
                        let mut all_files = Vec::new();
                        for folder in folders {
                            all_files.extend(scan_folder_for_media(std::path::Path::new(&folder.path)));
                        }
                        
                        if !all_files.is_empty() {
                            let client = std::sync::Arc::new(sync::ImmichClient::new(creds.server_url, creds.api_key));
                            let _ = run_sync_pipeline(handle_sync, pool, client, all_files, true).await;
                        }
                    }
                }
            });

            // Background Task to handle Watcher Events (Batched + debounced)
            let handle_task = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut paths_buffer = std::collections::HashSet::new();
                while let Some(event) = rx.recv().await {
                    for p in event.paths {
                        if p.is_dir() {
                            // If a directory is created/modified, scan it recursively
                            for (file_path, _size) in scan_folder_for_media(&p) {
                                paths_buffer.insert(file_path);
                            }
                        } else if p.is_file() && is_media_file(&p) {
                            if let Some(s) = p.to_str() {
                                paths_buffer.insert(s.to_string());
                            }
                        }
                    }

                    // Debounce: wait for 2s of silence before processing
                    let timeout = tokio::time::Duration::from_secs(2);
                    loop {
                        match tokio::time::timeout(timeout, rx.recv()).await {
                            Ok(Some(next_event)) => {
                                for p in next_event.paths {
                                    if p.is_dir() {
                                        for (file_path, _size) in scan_folder_for_media(&p) {
                                            paths_buffer.insert(file_path);
                                        }
                                    } else if p.is_file() && is_media_file(&p) {
                                        if let Some(s) = p.to_str() {
                                            paths_buffer.insert(s.to_string());
                                        }
                                    }
                                }
                            }
                            _ => break, // Timeout or channel closed
                        }
                    }

                    if !paths_buffer.is_empty() {
                        let paths: Vec<String> = paths_buffer.drain().collect();
                        let mut files = Vec::new();
                        for p in paths {
                            if let Ok(m) = std::fs::metadata(&p) {
                                files.push((p, m.len()));
                            }
                        }

                        let pool = handle_task.state::<sqlx::SqlitePool>();
                        let pool_inner = pool.inner().clone();

                        if let Ok(Some(creds)) = auth::get_credentials() {
                            let client = std::sync::Arc::new(sync::ImmichClient::new(
                                creds.server_url,
                                creds.api_key,
                            ));
                            let _ = run_sync_pipeline(handle_task.clone(), pool_inner, client, files, true).await;
                        }
                    }
                }
            });

            // Setup Tray Icon
            // Fix 14: greet() command removed (scaffold dead code)
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let sync_i = MenuItem::with_id(app, "sync", "Sync Now", true, None::<&str>)?;
            let menu_items: &[&dyn tauri::menu::IsMenuItem<tauri::Wry>] = &[
                &sync_i,
                &show_i,
                &quit_i,
            ];
            let menu = Menu::with_items(app, menu_items)?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
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
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .show_menu_on_left_click(false)
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Fix 14: greet removed
            login,
            logout,
            get_auth_status,
            get_server_url,
            add_folder,
            get_folders,
            remove_folder,
            start_sync
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
