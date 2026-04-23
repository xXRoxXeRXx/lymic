use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::fs;
use tauri::AppHandle;
use tauri::Manager;

pub async fn init(app_handle: &AppHandle) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let app_dir = app_handle.path().app_data_dir()?;
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)?;
    }
    let db_path = app_dir.join("immich_sync.db");

    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    // Create tables if they don't exist
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS watched_folders (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            recursive BOOLEAN NOT NULL DEFAULT 1,
            target_album_id TEXT
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_state (
            local_path TEXT PRIMARY KEY,
            file_hash TEXT NOT NULL,
            last_modified INTEGER NOT NULL,
            size INTEGER NOT NULL,
            status TEXT NOT NULL,
            remote_id TEXT
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub recursive: bool,
    pub target_album_id: Option<String>,
}

pub async fn add_folder(pool: &SqlitePool, path: &str) -> Result<i64, sqlx::Error> {
    let result = sqlx::query("INSERT INTO watched_folders (path) VALUES (?)")
        .bind(path)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_folders(pool: &SqlitePool) -> Result<Vec<WatchedFolder>, sqlx::Error> {
    sqlx::query_as::<_, WatchedFolder>("SELECT * FROM watched_folders")
        .fetch_all(pool)
        .await
}

pub async fn remove_folder(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watched_folders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_cached_hash(pool: &SqlitePool, path: &str, mtime: i64, size: i64) -> Option<String> {
    let result = sqlx::query(
        "SELECT file_hash FROM sync_state WHERE local_path = ? AND last_modified = ? AND size = ?",
    )
    .bind(path)
    .bind(mtime)
    .bind(size)
    .fetch_optional(pool)
    .await;

    match result {
        Ok(Some(row)) => {
            use sqlx::Row;
            Some(row.get(0))
        }
        _ => None,
    }
}

pub async fn update_sync_state(
    pool: &SqlitePool,
    path: &str,
    hash: &str,
    mtime: i64,
    size: i64,
    status: &str,
    remote_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sync_state (local_path, file_hash, last_modified, size, status, remote_id)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(local_path) DO UPDATE SET
            file_hash = excluded.file_hash,
            last_modified = excluded.last_modified,
            size = excluded.size,
            status = excluded.status,
            remote_id = excluded.remote_id",
    )
    .bind(path)
    .bind(hash)
    .bind(mtime)
    .bind(size)
    .bind(status)
    .bind(remote_id)
    .execute(pool)
    .await?;
    Ok(())
}
