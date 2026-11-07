use crate::{auth, immich::ImmichClient};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tauri::{
    http::{header, Response, StatusCode},
    Manager,
};
use tokio::sync::Semaphore;

const CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(crate) fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let downloads = Arc::new(Semaphore::new(12));
    builder.register_asynchronous_uri_scheme_protocol(
        "lymic-thumb",
        move |context, request, responder| {
            let downloads = downloads.clone();
            let cache_dir = context
                .app_handle()
                .path()
                .app_cache_dir()
                .map(|root| root.join("thumbnail_cache"));
            let uri = request.uri().clone();
            tauri::async_runtime::spawn(async move {
                let response = match cache_dir {
                    Ok(cache_dir) => serve(uri, cache_dir, downloads).await,
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
    cache_dir: PathBuf,
    downloads: Arc<Semaphore>,
) -> Response<Vec<u8>> {
    let Some(asset_id) = asset_id(&uri) else {
        return error_response(StatusCode::BAD_REQUEST, "Invalid thumbnail URI".to_string());
    };
    if let Ok(Some(bytes)) = read_cached(&cache_dir, asset_id) {
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
    if let Ok(Some(bytes)) = read_cached(&cache_dir, asset_id) {
        drop(permit);
        return webp_response(bytes);
    }
    let result = fetch_thumbnail(asset_id).await;
    drop(permit);
    match result {
        Ok(bytes) => {
            let _ = write_cached(&cache_dir, asset_id, &bytes);
            webp_response(bytes)
        }
        Err(error) => error_response(StatusCode::BAD_GATEWAY, error),
    }
}

fn asset_id(uri: &tauri::http::Uri) -> Option<&str> {
    if uri.authority()?.as_str() != "asset" {
        return None;
    }
    uri.path().strip_prefix('/').filter(|id| {
        !id.is_empty()
            && !id.contains('/')
            && !id.contains("..")
            && !id.chars().any(char::is_whitespace)
    })
}

async fn fetch_thumbnail(asset_id: &str) -> Result<Vec<u8>, String> {
    let credentials = auth::get_credentials()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Not authenticated".to_string())?;
    ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?
    .get_thumbnail(asset_id)
    .await
}

fn cache_path(cache_dir: &Path, asset_id: &str) -> PathBuf {
    cache_dir.join(format!("{:x}.webp", Sha256::digest(asset_id.as_bytes())))
}
fn read_cached(cache_dir: &Path, asset_id: &str) -> Result<Option<Vec<u8>>, String> {
    let path = cache_path(cache_dir, asset_id);
    match fs::read(&path) {
        Ok(bytes) => {
            let _ = fs::File::open(&path).and_then(|file| file.set_modified(SystemTime::now()));
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn write_cached(cache_dir: &Path, asset_id: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(cache_dir).map_err(|error| error.to_string())?;
    let path = cache_path(cache_dir, asset_id);
    let temp = cache_dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())?;
    evict(cache_dir)
}
fn evict(cache_dir: &Path) -> Result<(), String> {
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
    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in entries {
        if total <= CACHE_LIMIT_BYTES {
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
    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("lymic-thumb-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn cache_key_does_not_expose_asset_id_and_reuses_bytes() {
        let dir = temp_dir();
        write_cached(&dir, "asset/unsafe", b"webp").unwrap();
        assert_eq!(
            read_cached(&dir, "asset/unsafe").unwrap(),
            Some(b"webp".to_vec())
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
        assert_eq!(
            asset_id(&"lymic-thumb://asset/id".parse().unwrap()),
            Some("id")
        );
    }
}
