use serde::Serialize;
use sqlx::SqlitePool;

pub const DEFAULT_UPLOAD_PARALLELISM: usize = 3;
pub const DEFAULT_VIRTUAL_LIBRARY_PATH: &str = "%USERPROFILE%\\Pictures\\Lymic";
pub const DEFAULT_THUMBNAIL_CACHE_CAP_BYTES: u64 = 250 * 1024 * 1024;
const MIN_THUMBNAIL_CACHE_CAP_BYTES: u64 = 16 * 1024 * 1024;
const MAX_THUMBNAIL_CACHE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MIN_UPLOAD_PARALLELISM: usize = 1;
const MAX_UPLOAD_PARALLELISM: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualLibraryStatus {
    pub enabled: bool,
    pub path: String,
    pub connection_status: crate::virtual_library::sync_root::ConnectionStatus,
}

/// Resolves the display default to a path Windows APIs can actually open.
/// `%USERPROFILE%` is shell syntax; filesystem APIs do not expand it.
pub fn default_virtual_library_path() -> String {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .map(|home| {
            home.join("Pictures")
                .join("Lymic")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| DEFAULT_VIRTUAL_LIBRARY_PATH.to_string())
}

pub async fn get_virtual_library_status(
    pool: &SqlitePool,
) -> Result<VirtualLibraryStatus, sqlx::Error> {
    let enabled = sqlx::query_scalar::<_, String>(
        "SELECT value FROM app_settings WHERE key = 'virtual_library_enabled'",
    )
    .fetch_optional(pool)
    .await?
    .is_some_and(|value| value == "true");
    let path = sqlx::query_scalar::<_, String>(
        "SELECT value FROM app_settings WHERE key = 'virtual_library_path'",
    )
    .fetch_optional(pool)
    .await?
    .filter(|value| !value.is_empty())
    .map(|value| {
        (value == DEFAULT_VIRTUAL_LIBRARY_PATH)
            .then(default_virtual_library_path)
            .unwrap_or(value)
    })
    .unwrap_or_else(default_virtual_library_path);

    Ok(VirtualLibraryStatus {
        enabled,
        path,
        connection_status: crate::virtual_library::sync_root::ConnectionStatus::Disconnected,
    })
}

pub async fn set_virtual_library_enabled(pool: &SqlitePool, enabled: bool) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('virtual_library_enabled', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn validate_upload_parallelism(value: i64) -> Option<usize> {
    let value = usize::try_from(value).ok()?;
    (MIN_UPLOAD_PARALLELISM..=MAX_UPLOAD_PARALLELISM)
        .contains(&value)
        .then_some(value)
}

pub async fn get_upload_parallelism(pool: &SqlitePool) -> Result<usize, sqlx::Error> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT value FROM app_settings WHERE key = 'upload_parallelism'",
    )
    .fetch_optional(pool)
    .await?;

    Ok(value
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(validate_upload_parallelism)
        .unwrap_or(DEFAULT_UPLOAD_PARALLELISM))
}

pub async fn set_upload_parallelism(pool: &SqlitePool, value: usize) -> Result<(), String> {
    if !(MIN_UPLOAD_PARALLELISM..=MAX_UPLOAD_PARALLELISM).contains(&value) {
        return Err(format!(
            "Upload parallelism must be between {} and {}.",
            MIN_UPLOAD_PARALLELISM, MAX_UPLOAD_PARALLELISM
        ));
    }

    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('upload_parallelism', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(value.to_string())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn validate_thumbnail_cache_cap_bytes(value: i64) -> Option<u64> {
    let value = u64::try_from(value).ok()?;
    (MIN_THUMBNAIL_CACHE_CAP_BYTES..=MAX_THUMBNAIL_CACHE_CAP_BYTES)
        .contains(&value)
        .then_some(value)
}

pub async fn get_thumbnail_cache_cap_bytes(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT value FROM app_settings WHERE key = 'thumbnail_cache_cap_bytes'",
    )
    .fetch_optional(pool)
    .await?;
    Ok(value
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(validate_thumbnail_cache_cap_bytes)
        .unwrap_or(DEFAULT_THUMBNAIL_CACHE_CAP_BYTES))
}

pub async fn set_thumbnail_cache_cap_bytes(pool: &SqlitePool, value: u64) -> Result<(), String> {
    if !(MIN_THUMBNAIL_CACHE_CAP_BYTES..=MAX_THUMBNAIL_CACHE_CAP_BYTES).contains(&value) {
        return Err("Thumbnail cache cap must be between 16 MB and 2 GB.".to_string());
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('thumbnail_cache_cap_bytes', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(value.to_string())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}
