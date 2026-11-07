use crate::immich::ImmichClient;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};
use tauri::{
    http::{header, Response, StatusCode},
    Manager,
};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};

const CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_CONCURRENT_DOWNLOADS: usize = 12;
const MAX_QUEUED_WAITERS: usize = 256;
const EVICTION_CHECK_INTERVAL_WRITES: u64 = 64;

struct ViewportPriorityPool {
    semaphore: Arc<Semaphore>,
    waiters: Mutex<VecDeque<oneshot::Sender<OwnedSemaphorePermit>>>,
}

struct PriorityPermit {
    permit: Option<OwnedSemaphorePermit>,
    pool: Arc<ViewportPriorityPool>,
}

impl ViewportPriorityPool {
    fn new(max_concurrent: usize) -> Arc<Self> {
        Arc::new(Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            waiters: Mutex::new(VecDeque::new()),
        })
    }

    async fn acquire(self: &Arc<Self>) -> Result<PriorityPermit, ()> {
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            return Ok(PriorityPermit {
                permit: Some(permit),
                pool: self.clone(),
            });
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self.waiters.lock().map_err(|_| ())?;
            if waiters.len() >= MAX_QUEUED_WAITERS {
                // Evict the oldest scrolled-past waiter so viewport thumbnails stay responsive.
                let _ = waiters.pop_front();
            }
            waiters.push_back(tx);
        }
        // Re-check in case a permit was released just before we queued our waiter.
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            self.dispatch_permit(permit);
        }
        let permit = rx.await.map_err(|_| ())?;
        Ok(PriorityPermit {
            permit: Some(permit),
            pool: self.clone(),
        })
    }

    fn dispatch_permit(&self, mut permit: OwnedSemaphorePermit) {
        if let Ok(mut waiters) = self.waiters.lock() {
            // Wake the most recently requested (viewport-visible) waiter first (LIFO).
            while let Some(waiter) = waiters.pop_back() {
                match waiter.send(permit) {
                    Ok(()) => return,
                    Err(returned_permit) => permit = returned_permit,
                }
            }
        }
        drop(permit);
    }
}

impl Drop for PriorityPermit {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            self.pool.dispatch_permit(permit);
        }
    }
}

pub(crate) fn clear_thumbnail_cache(app: &tauri::AppHandle) {
    if let Ok(root) = app.path().app_cache_dir() {
        let dir = root.join("thumbnail_cache");
        if dir.exists() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

pub(crate) fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let downloads = ViewportPriorityPool::new(MAX_CONCURRENT_DOWNLOADS);
    let write_counter = Arc::new(AtomicU64::new(0));
    builder.register_asynchronous_uri_scheme_protocol(
        "lymic-thumb",
        move |context, request, responder| {
            let downloads = downloads.clone();
            let write_counter = write_counter.clone();
            let cache_dir = context
                .app_handle()
                .path()
                .app_cache_dir()
                .map(|root| root.join("thumbnail_cache"));
            let uri = request.uri().clone();
            tauri::async_runtime::spawn(async move {
                let response = match cache_dir {
                    Ok(cache_dir) => serve(uri, cache_dir, downloads, write_counter).await,
                    Err(error) => {
                        error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    }
                };
                responder.respond(response);
            });
        },
    )
}

async fn serve(
    uri: tauri::http::Uri,
    cache_root: PathBuf,
    downloads: Arc<ViewportPriorityPool>,
    write_counter: Arc<AtomicU64>,
) -> Response<Vec<u8>> {
    let Some(asset_id) = asset_id(&uri).map(str::to_string) else {
        return error_response(StatusCode::BAD_REQUEST, "Invalid thumbnail URI".to_string());
    };
    let (client, credentials, account_id) = match ImmichClient::from_stored_credentials() {
        Ok(authenticated) => authenticated,
        Err(error) => return error_response(StatusCode::UNAUTHORIZED, error),
    };
    let cache_dir = account_cache_dir(&cache_root, &account_id);
    if let Ok(Ok(Some(bytes))) = tokio::task::spawn_blocking({
        let cache_dir = cache_dir.clone();
        let asset_id = asset_id.clone();
        move || read_cached(&cache_dir, &asset_id)
    })
    .await
    {
        return webp_response(bytes);
    }
    let permit = match downloads.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Thumbnail service is unavailable".to_string(),
            )
        }
    };
    if let Ok(Ok(Some(bytes))) = tokio::task::spawn_blocking({
        let cache_dir = cache_dir.clone();
        let asset_id = asset_id.clone();
        move || read_cached(&cache_dir, &asset_id)
    })
    .await
    {
        drop(permit);
        return webp_response(bytes);
    }
    let result = client.get_thumbnail(&asset_id).await;
    drop(permit);
    match result {
        Ok(bytes) if is_webp(&bytes) => {
            let should_evict = write_counter
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(EVICTION_CHECK_INTERVAL_WRITES);
            let _ = tokio::task::spawn_blocking({
                let bytes = bytes.clone();
                move || write_cached_with_eviction(&cache_dir, &asset_id, &bytes, should_evict)
            })
            .await;
            webp_response(bytes)
        }
        Ok(_) => error_response(
            StatusCode::BAD_GATEWAY,
            "Invalid thumbnail format returned by server".to_string(),
        ),
        Err(error) => error_response(
            StatusCode::BAD_GATEWAY,
            ImmichClient::redact_error(&error, &credentials),
        ),
    }
}

fn account_cache_dir(cache_root: &Path, account_id: &str) -> PathBuf {
    cache_root.join(format!("{:x}", Sha256::digest(account_id.as_bytes())))
}

fn asset_id(uri: &tauri::http::Uri) -> Option<&str> {
    super::asset_protocol::protocol_target(uri, &["asset"]).map(|(_, id)| id)
}

fn cache_path(cache_dir: &Path, asset_id: &str) -> PathBuf {
    cache_dir.join(format!("{:x}.webp", Sha256::digest(asset_id.as_bytes())))
}
fn read_cached(cache_dir: &Path, asset_id: &str) -> Result<Option<Vec<u8>>, String> {
    let path = cache_path(cache_dir, asset_id);
    match fs::read(&path) {
        Ok(bytes) => {
            // A partial or otherwise corrupt cache entry must behave like a
            // miss. Leaving it in place would permanently poison this asset.
            if !is_webp(&bytes) {
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
            let _ = fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(SystemTime::now()));
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}
#[cfg(test)]
fn write_cached(cache_dir: &Path, asset_id: &str, bytes: &[u8]) -> Result<(), String> {
    write_cached_with_eviction(cache_dir, asset_id, bytes, true)
}
fn write_cached_with_eviction(
    cache_dir: &Path,
    asset_id: &str,
    bytes: &[u8],
    run_eviction: bool,
) -> Result<(), String> {
    if !is_webp(bytes) {
        return Err("Invalid WebP thumbnail payload".to_string());
    }
    fs::create_dir_all(cache_dir).map_err(|error| error.to_string())?;
    let path = cache_path(cache_dir, asset_id);
    let temp = cache_dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())?;
    if run_eviction {
        evict_to_limit(cache_dir, CACHE_LIMIT_BYTES)?;
    }
    Ok(())
}
fn evict_to_limit(cache_dir: &Path, limit_bytes: u64) -> Result<(), String> {
    let mut entries = fs::read_dir(cache_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut total = entries.iter().map(|(_, size, _)| *size).sum::<u64>();
    if total <= limit_bytes {
        return Ok(());
    }
    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in entries {
        if total <= limit_bytes {
            break;
        }
        fs::remove_file(path).map_err(|error| error.to_string())?;
        total = total.saturating_sub(size);
    }
    Ok(())
}
fn webp_response(bytes: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CACHE_CONTROL, "public, max-age=31536000")
        .body(bytes)
        .unwrap()
}
fn error_response(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.into_bytes())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID_WEBP: &[u8] = b"RIFF\x04\x00\x00\x00WEBP";

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("lymic-thumb-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn cache_key_does_not_expose_asset_id_and_reuses_bytes() {
        let dir = temp_dir();
        write_cached(&dir, "asset/unsafe", VALID_WEBP).unwrap();
        assert_eq!(
            read_cached(&dir, "asset/unsafe").unwrap(),
            Some(VALID_WEBP.to_vec())
        );
        assert!(!fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("asset"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_non_asset_thumbnail_uris() {
        assert!(asset_id(&"lymic-thumb://other/id".parse().unwrap()).is_none());
        assert!(asset_id(&"lymic-thumb://localhost/other/id".parse().unwrap()).is_none());
        assert_eq!(
            asset_id(&"lymic-thumb://asset/id".parse().unwrap()),
            Some("id")
        );
        assert_eq!(
            asset_id(&"lymic-thumb://localhost/asset/id".parse().unwrap()),
            Some("id")
        );
    }
    #[test]
    fn removes_corrupt_cache_entries_before_refetching() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(cache_path(&dir, "asset"), b"not-webp").unwrap();
        assert_eq!(read_cached(&dir, "asset").unwrap(), None);
        assert!(!cache_path(&dir, "asset").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn evicts_least_recently_modified_thumbnails_when_exceeding_limit() {
        let dir = temp_dir();
        write_cached_with_eviction(&dir, "older", VALID_WEBP, false).unwrap();
        let older_path = cache_path(&dir, "older");
        fs::OpenOptions::new()
            .write(true)
            .open(&older_path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10))
            .unwrap();
        write_cached_with_eviction(&dir, "newer", VALID_WEBP, false).unwrap();
        evict_to_limit(&dir, VALID_WEBP.len() as u64).unwrap();
        assert!(!older_path.exists());
        assert!(cache_path(&dir, "newer").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn priority_pool_wakes_most_recent_waiter_first() {
        let pool = ViewportPriorityPool::new(1);
        let first = pool.acquire().await.unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));

        let pool_old = pool.clone();
        let order_old = order.clone();
        let old_task = tokio::spawn(async move {
            let _permit = pool_old.acquire().await.unwrap();
            order_old.lock().unwrap().push("old");
        });
        tokio::task::yield_now().await;

        let pool_new = pool.clone();
        let order_new = order.clone();
        let new_task = tokio::spawn(async move {
            let _permit = pool_new.acquire().await.unwrap();
            order_new.lock().unwrap().push("new");
        });
        tokio::task::yield_now().await;

        drop(first);
        let _ = tokio::join!(old_task, new_task);
        assert_eq!(*order.lock().unwrap(), vec!["new", "old"]);
    }
}
