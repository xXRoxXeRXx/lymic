use sqlx::SqlitePool;

pub const DEFAULT_UPLOAD_PARALLELISM: usize = 3;
const MIN_UPLOAD_PARALLELISM: usize = 1;
const MAX_UPLOAD_PARALLELISM: usize = 8;

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

const VIRTUAL_LIBRARY_ENABLED_KEY: &str = "virtual_library_enabled";

async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
}

async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)          ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// The persisted Virtual Library toggle. Missing or unrecognised values count as disabled.
pub async fn get_virtual_library_enabled(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(get_setting(pool, VIRTUAL_LIBRARY_ENABLED_KEY)
        .await?
        .as_deref()
        == Some("1"))
}

pub async fn set_virtual_library_enabled(
    pool: &SqlitePool,
    enabled: bool,
) -> Result<(), sqlx::Error> {
    set_setting(
        pool,
        VIRTUAL_LIBRARY_ENABLED_KEY,
        if enabled { "1" } else { "0" },
    )
    .await
}
