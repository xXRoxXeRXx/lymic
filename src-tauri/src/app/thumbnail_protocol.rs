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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThumbnailKind {
    Asset,
    Person,
}

impl ThumbnailKind {
    fn cache_key(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Person => "person",
        }
    }
}

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
    let Some((kind, id)) = thumbnail_target(&uri).map(|(kind, id)| (kind, id.to_string())) else {
        return error_response(StatusCode::BAD_REQUEST, "Invalid thumbnail URI".to_string());
    };
    let cache_id = cache_identity(kind, &id, thumbnail_version(&uri));
    let (client, credentials, account_id) = match ImmichClient::from_stored_credentials() {
        Ok(authenticated) => authenticated,
        Err(error) => return error_response(StatusCode::UNAUTHORIZED, error),
    };
    let cache_dir = account_cache_dir(&cache_root, &account_id);
    if let Ok(Ok(Some(bytes))) = tokio::task::spawn_blocking({
        let cache_dir = cache_dir.clone();
        let cache_id = cache_id.clone();
        move || read_cached(&cache_dir, kind, &cache_id)
    })
    .await
    {
        return image_response(bytes);
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
        let cache_id = cache_id.clone();
        move || read_cached(&cache_dir, kind, &cache_id)
    })
    .await
    {
        drop(permit);
        return image_response(bytes);
    }
    let result = match kind {
        ThumbnailKind::Asset => client.get_thumbnail(&id).await,
        ThumbnailKind::Person => client.get_person_thumbnail(&id).await,
    };
    drop(permit);
    match result {
        Ok(bytes) if is_valid_image(kind, &bytes) => {
            let should_evict = write_counter
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(EVICTION_CHECK_INTERVAL_WRITES);
            let _ = tokio::task::spawn_blocking({
                let bytes = bytes.clone();
                move || write_cached_with_eviction(&cache_dir, kind, &cache_id, &bytes, should_evict)
            })
            .await;
            image_response(bytes)
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

/// Person thumbnails are regenerated by Immich under a new `thumbnailPath`, so the
/// UI passes that path as `?v=` and it becomes part of the cache identity.
fn thumbnail_version(uri: &tauri::http::Uri) -> Option<&str> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("v="))
        .filter(|version| !version.is_empty() && version.len() <= 512)
}

fn cache_identity(kind: ThumbnailKind, id: &str, version: Option<&str>) -> String {
    match (kind, version) {
        (ThumbnailKind::Person, Some(version)) => format!("{id}@{version}"),
        _ => id.to_string(),
    }
}

fn account_cache_dir(cache_root: &Path, account_id: &str) -> PathBuf {
    cache_root.join(format!("{:x}", Sha256::digest(account_id.as_bytes())))
}

fn thumbnail_target(uri: &tauri::http::Uri) -> Option<(ThumbnailKind, &str)> {
    let (kind, id) = super::asset_protocol::protocol_target(uri, &["asset", "person"])?;
    Some((
        match kind {
            "asset" => ThumbnailKind::Asset,
            "person" => ThumbnailKind::Person,
            _ => return None,
        },
        id,
    ))
}

#[cfg(test)]
fn asset_id(uri: &tauri::http::Uri) -> Option<&str> {
    thumbnail_target(uri).and_then(|(kind, id)| (kind == ThumbnailKind::Asset).then_some(id))
}
fn cache_path(cache_dir: &Path, kind: ThumbnailKind, id: &str) -> PathBuf {
    cache_dir.join(format!(
        "{:x}.image",
        Sha256::digest(format!("{}:{id}", kind.cache_key()).as_bytes())
    ))
}
fn read_cached(cache_dir: &Path, kind: ThumbnailKind, id: &str) -> Result<Option<Vec<u8>>, String> {
    let path = cache_path(cache_dir, kind, id);
    match fs::read(&path) {
        Ok(bytes) => {
            // A partial or otherwise corrupt cache entry must behave like a
            // miss. Leaving it in place would permanently poison this asset.
            if !is_valid_image(kind, &bytes) {
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
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(&[0xFF, 0xD8]) && bytes.ends_with(&[0xFF, 0xD9])
}
fn is_valid_image(kind: ThumbnailKind, bytes: &[u8]) -> bool {
    is_webp(bytes) || (kind == ThumbnailKind::Person && is_jpeg(bytes))
}
#[cfg(test)]
fn write_cached(
    cache_dir: &Path,
    kind: ThumbnailKind,
    id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    write_cached_with_eviction(cache_dir, kind, id, bytes, true)
}
fn write_cached_with_eviction(
    cache_dir: &Path,
    kind: ThumbnailKind,
    id: &str,
    bytes: &[u8],
    run_eviction: bool,
) -> Result<(), String> {
    if !is_valid_image(kind, bytes) {
        return Err("Invalid thumbnail payload".to_string());
    }
    fs::create_dir_all(cache_dir).map_err(|error| error.to_string())?;
    let path = cache_path(cache_dir, kind, id);
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
fn image_response(bytes: Vec<u8>) -> Response<Vec<u8>> {
    let content_type = if is_webp(&bytes) {
        "image/webp"
    } else {
        "image/jpeg"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
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
    const VALID_JPEG: &[u8] = b"\xFF\xD8person-thumbnail\xFF\xD9";

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("lymic-thumb-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn cache_key_does_not_expose_asset_id_and_reuses_bytes() {
        let dir = temp_dir();
        write_cached(&dir, ThumbnailKind::Asset, "asset/unsafe", VALID_WEBP).unwrap();
        assert_eq!(
            read_cached(&dir, ThumbnailKind::Asset, "asset/unsafe").unwrap(),
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
    fn accepts_only_known_thumbnail_uri_targets() {
        assert!(asset_id(&"lymic-thumb://other/id".parse().unwrap()).is_none());
        assert!(thumbnail_target(&"lymic-thumb://localhost/other/id".parse().unwrap()).is_none());
        assert_eq!(
            asset_id(&"lymic-thumb://asset/id".parse().unwrap()),
            Some("id")
        );
        assert_eq!(
            asset_id(&"lymic-thumb://localhost/asset/id".parse().unwrap()),
            Some("id")
        );
        assert_eq!(
            thumbnail_target(&"lymic-thumb://person/person-1".parse().unwrap()),
            Some((ThumbnailKind::Person, "person-1"))
        );
        assert!(thumbnail_target(&"lymic-thumb://person/not/a-person".parse().unwrap()).is_none());
    }
    #[test]
    fn person_cache_identity_changes_with_thumbnail_version() {
        let old: tauri::http::Uri = "lymic-thumb://person/p1?v=%2Fa.jpg".parse().unwrap();
        let new: tauri::http::Uri = "lymic-thumb://person/p1?v=%2Fb.jpg".parse().unwrap();
        assert_ne!(
            cache_identity(ThumbnailKind::Person, "p1", thumbnail_version(&old)),
            cache_identity(ThumbnailKind::Person, "p1", thumbnail_version(&new))
        );
        assert_eq!(cache_identity(ThumbnailKind::Asset, "a1", Some("x")), "a1");
        assert!(thumbnail_target(&old).is_some());
    }

    #[test]
    fn separates_person_and_asset_cache_identities() {
        let dir = temp_dir();
        assert_ne!(
            cache_path(&dir, ThumbnailKind::Asset, "shared-id"),
            cache_path(&dir, ThumbnailKind::Person, "shared-id")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn reuses_valid_person_thumbnail_cache_entries() {
        let dir = temp_dir();
        write_cached(&dir, ThumbnailKind::Person, "person-1", VALID_JPEG).unwrap();
        assert_eq!(
            read_cached(&dir, ThumbnailKind::Person, "person-1").unwrap(),
            Some(VALID_JPEG.to_vec())
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn removes_corrupt_cache_entries_before_refetching() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(cache_path(&dir, ThumbnailKind::Asset, "asset"), b"not-webp").unwrap();
        assert_eq!(
            read_cached(&dir, ThumbnailKind::Asset, "asset").unwrap(),
            None
        );
        assert!(!cache_path(&dir, ThumbnailKind::Asset, "asset").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn evicts_least_recently_modified_thumbnails_when_exceeding_limit() {
        let dir = temp_dir();
        write_cached_with_eviction(&dir, ThumbnailKind::Asset, "older", VALID_WEBP, false).unwrap();
        let older_path = cache_path(&dir, ThumbnailKind::Asset, "older");
        fs::OpenOptions::new()
            .write(true)
            .open(&older_path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10))
            .unwrap();
        write_cached_with_eviction(&dir, ThumbnailKind::Asset, "newer", VALID_WEBP, false).unwrap();
        evict_to_limit(&dir, VALID_WEBP.len() as u64).unwrap();
        assert!(!older_path.exists());
        assert!(cache_path(&dir, ThumbnailKind::Asset, "newer").exists());
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
