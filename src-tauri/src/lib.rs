mod auth;
mod db;
mod sync;
mod watcher;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri::{Emitter, Manager};
use futures::StreamExt;
use tauri_plugin_notification::NotificationExt;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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

#[tauri::command]
async fn login(app: tauri::AppHandle, server_url: String, api_key: String) -> Result<(), String> {
    log_to_ui(&app, "INFO", &format!("Attempting to connect to {}", server_url));
    
    let mut base_url = server_url.trim_end_matches('/').to_string();
    if !base_url.ends_with("/api") {
        base_url.push_str("/api");
    }

    // Validate connection by fetching server info
    let url = format!("{}/server-info/config", base_url);
    let response = reqwest::Client::new()
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
async fn add_folder(
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, std::sync::Mutex<notify::RecommendedWatcher>>,
    path: String,
) -> Result<i64, String> {
    let id = db::db::add_folder(&pool, &path)
        .await
        .map_err(|e| e.to_string())?;
    let mut w = watcher.lock().unwrap();
    let _ = watcher::watch_path(&mut w, &path);
    Ok(id)
}

#[tauri::command]
async fn get_folders(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<Vec<db::db::WatchedFolder>, String> {
    db::db::get_folders(&pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_folder(
    pool: tauri::State<'_, sqlx::SqlitePool>,
    watcher: tauri::State<'_, std::sync::Mutex<notify::RecommendedWatcher>>,
    id: i64,
) -> Result<(), String> {
    let folders = db::db::get_folders(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(folder) = folders.iter().find(|f| f.id == id) {
        let mut w = watcher.lock().unwrap();
        let _ = watcher::unwatch_path(&mut w, &folder.path);
    }
    db::db::remove_folder(&pool, id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_sync(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>
) -> Result<(), String> {
    log_to_ui(&app, "INFO", "Starting manual synchronization...");
    let folders = db::db::get_folders(&pool).await.map_err(|e| e.to_string())?;
    let credentials = auth::get_credentials()?.ok_or("Not logged in")?;
    
    let client = std::sync::Arc::new(sync::ImmichClient::new(credentials.server_url, credentials.api_key));
    let device_id = format!("{}-IMMICH-DESKTOP", std::env::consts::OS.to_uppercase());

    let mut all_files = Vec::new();
    for folder in folders {
        log_to_ui(&app, "INFO", &format!("Scanning folder: {}", folder.path));
        let path = std::path::Path::new(&folder.path);
        if !path.exists() { 
            log_to_ui(&app, "WARN", &format!("Path does not exist: {}", folder.path));
            continue; 
        }

        let walker = walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok());
        for entry in walker {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    let ext = ext.to_lowercase();
                    if ["jpg", "jpeg", "png", "gif", "heic", "heif", "mp4", "mov", "avi"].contains(&ext.as_str()) {
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        all_files.push((entry.path().to_string_lossy().to_string(), size));
                    }
                }
            }
        }
    }

    let total_bytes: u64 = all_files.iter().map(|(_, size)| size).sum();
    let file_count = all_files.len();
    let completed_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    log_to_ui(&app, "INFO", &format!("Found {} files to check ({:.2} MB)", file_count, total_bytes as f64 / 1024.0 / 1024.0));
    let app_handle = std::sync::Arc::new(app.clone());
    let pool_inner = pool.inner().clone();
    
    log_to_ui(&app, "INFO", "Verifying assets with server...");

    // 1. Calculate all hashes (cached by DB) and prepare for bulk check
    let mut file_data = Vec::new();
    for (path, size) in all_files {
        let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        let mtime = metadata.modified().unwrap_or_else(|_| metadata.created().unwrap())
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let size_i64 = size as i64;

        let hash = if let Some(cached) = db::db::get_cached_hash(&pool_inner, &path, mtime, size_i64).await {
            cached
        } else {
            let h = sync::calculate_hash(&path).map_err(|e| e.to_string())?;
            // Update cache with new hash
            let _ = db::db::update_sync_state(&pool_inner, &path, &h, mtime, size_i64, "PENDING", None).await;
            h
        };
        file_data.push((path, size, hash, mtime));
    }

    if file_data.is_empty() {
        log_to_ui(&app, "INFO", "No media files found.");
        return Ok(());
    }

    // 2. Bulk check in chunks of 500
    let mut missing_files = Vec::new();
    for chunk in file_data.chunks(500) {
        let hashes: Vec<String> = chunk.iter().map(|(_, _, h, _)| h.clone()).collect();
        match client.check_assets_exist(hashes).await {
            Ok(existing_hashes) => {
                for (path, size, hash, mtime) in chunk {
                    if !existing_hashes.contains(hash) {
                        missing_files.push((path.clone(), *size, hash.clone(), *mtime));
                    } else {
                        // Mark as synced in local DB if server has it
                        let _ = db::db::update_sync_state(&pool_inner, path, hash, *mtime, *size as i64, "SYNCED", None).await;
                        let done = completed_bytes.fetch_add(*size, std::sync::atomic::Ordering::SeqCst) + *size;
                        let pct = (done as f64 / total_bytes as f64 * 100.0) as u32;
                        let _ = app.emit("sync-progress-percent", pct);
                    }
                }
            }
            Err(e) => {
                log_to_ui(&app, "ERROR", &format!("Bulk check failed: {}", e));
            }
        }
    }

    let to_upload_count = missing_files.len();
    log_to_ui(&app, "INFO", &format!("Found {} files to upload/restore.", to_upload_count));

    // 3. Upload missing files
    futures::stream::iter(missing_files)
        .map(|(file_path, file_size, hash, mtime)| {
            let client = client.clone();
            let device_id = device_id.clone();
            let app = app_handle.clone();
            let pool_clone = pool_inner.clone();
            let completed_bytes = completed_bytes.clone();
            async move {
                let _ = app.emit("sync-progress", &file_path);
                log_to_ui(&app, "INFO", &format!("Uploading: {}", file_path));
                match client.upload_asset(&file_path, &device_id).await {
                    Ok(remote_id) => {
                        log_to_ui(&app, "SUCCESS", &format!("Uploaded {}", file_path));
                        let _ = db::db::update_sync_state(&pool_clone, &file_path, &hash, mtime, file_size as i64, "SYNCED", Some(&remote_id)).await;
                    }
                    Err(e) => {
                        log_to_ui(&app, "ERROR", &format!("Failed to upload {}: {}", file_path, e));
                        let _ = db::db::update_sync_state(&pool_clone, &file_path, &hash, mtime, file_size as i64, "FAILED", None).await;
                    }
                }
                
                let done = completed_bytes.fetch_add(file_size, std::sync::atomic::Ordering::SeqCst) + file_size;
                let pct = (done as f64 / total_bytes as f64 * 100.0) as u32;
                let _ = app.emit("sync-progress-percent", pct);
            }
        })
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await;
    
    send_notification(&app, "Sync Complete", &format!("Successfully processed {} files.", file_count));
    Ok(())
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
            let mut watcher = watcher::create_watcher(tx).map_err(|e| e.to_string())?;

            // Initialize Database and Watch folders
            let handle_clone = handle.clone();
            tauri::async_runtime::block_on(async move {
                match db::db::init(&handle_clone).await {
                    Ok(pool) => {
                        // Watch existing folders
                        if let Ok(folders) = db::db::get_folders(&pool).await {
                            for folder in folders {
                                let _ = watcher::watch_path(&mut watcher, &folder.path);
                            }
                        }
                        handle_clone.manage(pool);
                        handle_clone.manage(std::sync::Mutex::new(watcher));
                    }
                    Err(e) => {
                        eprintln!("Failed to initialize database: {}", e);
                    }
                }
            });

            // Background Task to handle Watcher Events (Batched)
            let handle_task = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut paths_buffer = std::collections::HashSet::new();
                while let Some(event) = rx.recv().await {
                    for p in event.paths {
                        if let Some(s) = p.to_str() { 
                            let path_buf = std::path::Path::new(s);
                            if path_buf.is_file() {
                                paths_buffer.insert(s.to_string()); 
                            }
                        }
                    }

                    // Debounce: Wait for 2s of silence before processing
                    let timeout = tokio::time::Duration::from_secs(2);
                    loop {
                        match tokio::time::timeout(timeout, rx.recv()).await {
                            Ok(Some(next_event)) => {
                                for p in next_event.paths {
                                    if let Some(s) = p.to_str() { 
                                        let path_buf = std::path::Path::new(s);
                                        if path_buf.is_file() {
                                            paths_buffer.insert(s.to_string()); 
                                        }
                                    }
                                }
                            }
                            _ => break, // Timeout reached or channel closed
                        }
                    }

                    if !paths_buffer.is_empty() {
                        let paths: Vec<String> = paths_buffer.drain().collect();
                        let total_files = paths.len();
                        log_to_ui(&handle_task, "INFO", &format!("Auto-sync batch started: {} files", total_files));
                        
                        let pool = handle_task.state::<sqlx::SqlitePool>();
                        let pool_inner = pool.inner().clone(); 
                        if let Ok(Some(creds)) = auth::get_credentials() {
                            let client = std::sync::Arc::new(sync::ImmichClient::new(creds.server_url, creds.api_key));
                            let device_id = format!("{}-IMMICH-DESKTOP", std::env::consts::OS.to_uppercase());
                            let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

                            let _ = handle_task.emit("sync-progress-percent", 0);

                            futures::stream::iter(paths)
                                .map(|path_str: String| {
                                    let client = client.clone();
                                    let device_id = device_id.clone();
                                    let handle = handle_task.clone();
                                    let pool = pool_inner.clone();
                                    let completed = completed_count.clone();
                                    async move {
                                        let _ = handle.emit("sync-progress", &path_str);
                                        
                                        let metadata = std::fs::metadata(&path_str).ok();
                                        let mtime = metadata.as_ref().map(|m| m.modified().unwrap_or_else(|_| m.created().unwrap()).duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64).unwrap_or(0);
                                        let size = metadata.as_ref().map(|m| m.len() as i64).unwrap_or(0);

                                        let hash = if let Some(cached) = db::db::get_cached_hash(&pool, &path_str, mtime, size).await {
                                            cached
                                        } else if let Ok(h) = sync::calculate_hash(&path_str) {
                                            h
                                        } else {
                                            return;
                                        };

                                        match client.check_assets_exist(vec![hash.clone()]).await {
                                            Ok(existing) => {
                                                if existing.is_empty() {
                                                    match client.upload_asset(&path_str, &device_id).await {
                                                        Ok(remote_id) => {
                                                            let _ = db::db::update_sync_state(&pool, &path_str, &hash, mtime, size, "SYNCED", Some(&remote_id)).await;
                                                        }
                                                        Err(e) => {
                                                            log_to_ui(&handle, "ERROR", &format!("Auto-sync failed: {}", e));
                                                            let _ = db::db::update_sync_state(&pool, &path_str, &hash, mtime, size, "FAILED", None).await;
                                                        }
                                                    }
                                                } else {
                                                    let _ = db::db::update_sync_state(&pool, &path_str, &hash, mtime, size, "SYNCED", None).await;
                                                }
                                            }
                                            Err(_) => {}
                                        }
                                        
                                        let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                                        let pct = (done as f64 / total_files as f64 * 100.0) as u32;
                                        let _ = handle.emit("sync-progress-percent", pct);
                                    }
                                })
                                .buffer_unordered(2)
                                .collect::<Vec<_>>()
                                .await;
                            
                            let _ = handle_task.emit("sync-idle", ());
                        }
                    }
                }
            });

            // Setup Tray Icon
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let sync_i = MenuItem::with_id(app, "sync", "Sync Now", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&sync_i, &show_i, &quit_i])?;

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
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
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
            greet,
            login,
            logout,
            get_auth_status,
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
