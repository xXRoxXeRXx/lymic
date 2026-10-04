use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;
use tauri::Manager;

pub const DEFAULT_UPLOAD_PARALLELISM: usize = 3;
const MIN_UPLOAD_PARALLELISM: usize = 1;
const MAX_UPLOAD_PARALLELISM: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseStatus {
    Ready,
    Repaired {
        backup_path: PathBuf,
        salvaged_folders: usize,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct DbInitResult {
    pub pool: SqlitePool,
    pub status: DatabaseStatus,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedSyncEntry {
    pub local_path: String,
    pub failure_reason: String,
}

#[derive(Debug)]
pub(crate) enum InitFailure {
    Corrupt(String),
    Other(Box<dyn std::error::Error + Send + Sync>),
}

pub(crate) fn detect_corruption(err: &sqlx::Error) -> Option<String> {
    if let Some(db_err) = err.as_database_error() {
        if let Some(code_str) = db_err.code() {
            if let Ok(code) = code_str.parse::<i32>() {
                let primary = code & 0xFF;
                if primary == 11 || primary == 26 {
                    return Some(format!(
                        "SQLite corruption error (code {}): {}",
                        code,
                        db_err.message()
                    ));
                }
            }
        }
    }
    None
}

pub(crate) fn classify_migrate_error(err: sqlx::migrate::MigrateError) -> InitFailure {
    let mut current_source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(src) = current_source {
        if let Some(sqlx_err) = src.downcast_ref::<sqlx::Error>() {
            if let Some(reason) = detect_corruption(sqlx_err) {
                return InitFailure::Corrupt(reason);
            }
        }
        current_source = src.source();
    }
    InitFailure::Other(Box::new(err))
}

async fn open_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    // SQLite permits one writer at a time; WAL keeps readers responsive while writes wait.
    SqlitePoolOptions::new()
        .max_connections(4)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
}

async fn verify_and_migrate(pool: &SqlitePool) -> Result<(), InitFailure> {
    let row = sqlx::query("PRAGMA quick_check(1)")
        .fetch_one(pool)
        .await
        .map_err(|e| match detect_corruption(&e) {
            Some(reason) => InitFailure::Corrupt(reason),
            None => InitFailure::Other(Box::new(e)),
        })?;

    let status: String = row
        .try_get(0)
        .map_err(|e| InitFailure::Other(Box::new(e)))?;
    if !status.eq_ignore_ascii_case("ok") {
        return Err(InitFailure::Corrupt(format!(
            "SQLite quick_check reported corruption: {}",
            status
        )));
    }

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(classify_migrate_error)?;

    Ok(())
}

async fn try_salvage_folders(db_path: &Path) -> Vec<WatchedFolder> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .read_only(true)
        .busy_timeout(Duration::from_secs(2));

    let pool = match SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect_with(options)
        .await
    {
        Ok(pool) => pool,
        Err(_) => return Vec::new(),
    };

    let folders = get_folders(&pool).await.unwrap_or_default();
    pool.close().await;
    folders
}

async fn move_or_copy_and_remove(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    if !src.exists() {
        return Ok(());
    }

    let mut last_err = None;
    for _ in 0..5 {
        match fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    // Fallback: copy then remove
    fs::copy(src, dst).map_err(|e| last_err.unwrap_or(e))?;
    fs::remove_file(src)?;
    Ok(())
}

async fn quarantine_corrupt_db(db_path: &Path) -> Result<PathBuf, std::io::Error> {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("immich_sync.db");
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");

    let mut candidate_name = format!("{}.corrupted.{}", file_name, timestamp);
    let mut candidate_path = parent.join(&candidate_name);
    let mut counter = 1;
    while candidate_path.exists()
        || parent.join(format!("{}-wal", candidate_name)).exists()
        || parent.join(format!("{}-shm", candidate_name)).exists()
    {
        candidate_name = format!("{}.corrupted.{}_{}", file_name, timestamp, counter);
        candidate_path = parent.join(&candidate_name);
        counter += 1;
    }
    let backup_path = candidate_path;
    let backup_name = candidate_name;

    // Quarantine main db file
    move_or_copy_and_remove(db_path, &backup_path).await?;

    // Quarantine -wal and -shm files if they exist
    let wal_path = parent.join(format!("{}-wal", file_name));
    let backup_wal = parent.join(format!("{}-wal", backup_name));
    move_or_copy_and_remove(&wal_path, &backup_wal).await?;

    let shm_path = parent.join(format!("{}-shm", file_name));
    let backup_shm = parent.join(format!("{}-shm", backup_name));
    move_or_copy_and_remove(&shm_path, &backup_shm).await?;

    Ok(backup_path)
}

async fn repair_corrupt_db(
    db_path: &Path,
    reason: String,
) -> Result<DbInitResult, Box<dyn std::error::Error + Send + Sync>> {
    let salvaged = try_salvage_folders(db_path).await;
    let backup_path = quarantine_corrupt_db(db_path).await?;

    let new_pool = open_pool(db_path).await?;
    match verify_and_migrate(&new_pool).await {
        Ok(()) => {}
        Err(InitFailure::Corrupt(r)) => {
            return Err(format!("Newly created database was also corrupt: {}", r).into())
        }
        Err(InitFailure::Other(e)) => return Err(e),
    }

    let mut restored_count = 0;
    for folder in salvaged {
        if restore_folder(
            &new_pool,
            &folder.path,
            folder.recursive,
            folder.target_album_id.as_deref(),
        )
        .await
        .is_ok()
        {
            restored_count += 1;
        }
    }

    Ok(DbInitResult {
        pool: new_pool,
        status: DatabaseStatus::Repaired {
            backup_path,
            salvaged_folders: restored_count,
            reason,
        },
    })
}

pub async fn init_at_path(
    db_path: &Path,
) -> Result<DbInitResult, Box<dyn std::error::Error + Send + Sync>> {
    if db_path.exists() {
        match open_pool(db_path).await {
            Ok(pool) => match verify_and_migrate(&pool).await {
                Ok(()) => Ok(DbInitResult {
                    pool,
                    status: DatabaseStatus::Ready,
                }),
                Err(InitFailure::Corrupt(reason)) => {
                    pool.close().await;
                    repair_corrupt_db(db_path, reason).await
                }
                Err(InitFailure::Other(err)) => {
                    pool.close().await;
                    Err(err)
                }
            },
            Err(e) => match detect_corruption(&e) {
                Some(reason) => repair_corrupt_db(db_path, reason).await,
                None => Err(Box::new(e)),
            },
        }
    } else {
        let pool = open_pool(db_path).await?;
        match verify_and_migrate(&pool).await {
            Ok(()) => Ok(DbInitResult {
                pool,
                status: DatabaseStatus::Ready,
            }),
            Err(InitFailure::Corrupt(reason)) => {
                pool.close().await;
                repair_corrupt_db(db_path, reason).await
            }
            Err(InitFailure::Other(err)) => {
                pool.close().await;
                Err(err)
            }
        }
    }
}

pub async fn init(
    app_handle: &AppHandle,
) -> Result<DbInitResult, Box<dyn std::error::Error + Send + Sync>> {
    let app_dir = app_handle.path().app_data_dir()?;
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir)?;
    }
    let db_path = app_dir.join("immich_sync.db");
    init_at_path(&db_path).await
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

pub async fn restore_folder(
    pool: &SqlitePool,
    path: &str,
    recursive: bool,
    target_album_id: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO watched_folders (path, recursive, target_album_id) VALUES (?, ?, ?)",
    )
    .bind(path)
    .bind(recursive)
    .bind(target_album_id)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_folders(pool: &SqlitePool) -> Result<Vec<WatchedFolder>, sqlx::Error> {
    sqlx::query_as::<_, WatchedFolder>(
        "SELECT id, path, recursive, target_album_id FROM watched_folders",
    )
    .fetch_all(pool)
    .await
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

pub async fn remove_folder(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watched_folders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_cached_hash(
    pool: &SqlitePool,
    path: &str,
    mtime: i64,
    size: i64,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT file_hash FROM sync_state
         WHERE local_path = ? AND last_modified = ? AND size = ?
            AND status = 'SYNCED' AND file_hash != ''",
    )
    .bind(path)
    .bind(mtime)
    .bind(size)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| row.get(0)))
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
             remote_id = excluded.remote_id,
             failure_reason = CASE WHEN excluded.status = 'SYNCED' THEN NULL ELSE sync_state.failure_reason END",
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

pub async fn mark_sync_failed(
    pool: &SqlitePool,
    path: &str,
    mtime: i64,
    size: i64,
    failure_reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sync_state (local_path, file_hash, last_modified, size, status, remote_id, failure_reason)
         VALUES (?, '', ?, ?, 'FAILED', NULL, ?)
         ON CONFLICT(local_path) DO UPDATE SET
             file_hash = excluded.file_hash,
             last_modified = excluded.last_modified,
             size = excluded.size,
             status = excluded.status,
             remote_id = NULL,
             failure_reason = excluded.failure_reason",
    )
    .bind(path)
    .bind(mtime)
    .bind(size)
    .bind(failure_reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_failed_syncs(pool: &SqlitePool) -> Result<Vec<FailedSyncEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT local_path, COALESCE(NULLIF(failure_reason, ''), 'Failure details are unavailable; retry to obtain details.') AS failure_reason
         FROM sync_state
         WHERE status = 'FAILED' OR (failure_reason IS NOT NULL AND failure_reason != '')
         ORDER BY local_path ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| FailedSyncEntry {
            local_path: row.get("local_path"),
            failure_reason: row.get("failure_reason"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{}?mode=memory&cache=private",
                database_name
            ))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn initial_migration_creates_schema() {
        let pool = test_pool().await;

        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        add_folder(&pool, "C:/photos").await.unwrap();
        update_sync_state(&pool, "photo.jpg", "hash", 42, 123, "SYNCED", None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn upload_parallelism_defaults_persists_and_validates_range() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );

        set_upload_parallelism(&pool, 6).await.unwrap();
        assert_eq!(get_upload_parallelism(&pool).await.unwrap(), 6);
        assert!(set_upload_parallelism(&pool, 0).await.is_err());
        assert!(set_upload_parallelism(&pool, 9).await.is_err());
    }

    #[tokio::test]
    async fn invalid_stored_upload_parallelism_uses_default() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "UPDATE app_settings SET value = 'not-a-number' WHERE key = 'upload_parallelism'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );

        sqlx::query("UPDATE app_settings SET value = '0' WHERE key = 'upload_parallelism'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );
    }

    #[tokio::test]
    async fn cached_hash_misses_when_mtime_differs_within_a_second() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        update_sync_state(
            &pool,
            "photo.jpg",
            "cached-hash",
            1_700_000_000_100_000_000,
            123,
            "SYNCED",
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            get_cached_hash(&pool, "photo.jpg", 1_700_000_000_200_000_000, 123)
                .await
                .unwrap(),
            None,
        );
    }

    #[tokio::test]
    async fn cached_hash_propagates_query_errors() {
        let pool = test_pool().await;

        assert!(get_cached_hash(&pool, "photo.jpg", 42, 123).await.is_err());
    }

    #[tokio::test]
    async fn failed_state_clears_hash_and_is_never_cached() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        update_sync_state(&pool, "photo.jpg", "old-hash", 42, 123, "SYNCED", None)
            .await
            .unwrap();
        mark_sync_failed(&pool, "photo.jpg", 42, 123, "Upload rejected")
            .await
            .unwrap();

        assert_eq!(
            get_cached_hash(&pool, "photo.jpg", 42, 123).await.unwrap(),
            None
        );

        let row = sqlx::query("SELECT file_hash, status FROM sync_state WHERE local_path = ?")
            .bind("photo.jpg")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("file_hash"), "");
        assert_eq!(row.get::<String, _>("status"), "FAILED");
    }

    #[tokio::test]
    async fn failed_sync_entries_persist_update_and_clear_on_success() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        mark_sync_failed(&pool, "b.jpg", 1, 2, "First error")
            .await
            .unwrap();
        mark_sync_failed(&pool, "a.jpg", 3, 4, "Other error")
            .await
            .unwrap();
        mark_sync_failed(&pool, "b.jpg", 5, 6, "Updated error")
            .await
            .unwrap();

        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].local_path, "a.jpg");
        assert_eq!(failures[1].failure_reason, "Updated error");

        update_sync_state(&pool, "b.jpg", "hash", 5, 6, "PENDING", None)
            .await
            .unwrap();
        assert_eq!(get_failed_syncs(&pool).await.unwrap().len(), 2);

        update_sync_state(&pool, "b.jpg", "hash", 5, 6, "SYNCED", None)
            .await
            .unwrap();
        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "a.jpg");
    }

    #[tokio::test]
    async fn failed_sync_entries_include_legacy_rows_without_a_reason() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO sync_state (local_path, file_hash, last_modified, size, status, remote_id, failure_reason)
             VALUES (?, '', 0, 0, 'FAILED', NULL, NULL)",
        )
        .bind("legacy.jpg")
        .execute(&pool)
        .await
        .unwrap();

        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "legacy.jpg");
        assert_eq!(
            failures[0].failure_reason,
            "Failure details are unavailable; retry to obtain details."
        );
    }

    #[tokio::test]
    async fn corrupt_database_is_quarantined_and_repaired() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("corrupt_test.db");

        // Write non-database invalid data to simulate corrupted SQLite file
        std::fs::write(
            &db_path,
            b"NOT A VALID SQLITE DATABASE FILE - CORRUPTED DATA",
        )
        .unwrap();

        let result = init_at_path(&db_path).await.unwrap();
        match result.status {
            DatabaseStatus::Repaired {
                backup_path,
                salvaged_folders,
                reason,
            } => {
                assert!(backup_path.exists());
                assert_eq!(salvaged_folders, 0);
                assert!(!reason.is_empty());
            }
            DatabaseStatus::Ready => panic!("Expected database to be repaired, but got Ready"),
        }

        // New database should be fully functional
        add_folder(&result.pool, "C:/test_photos").await.unwrap();
        let folders = get_folders(&result.pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "C:/test_photos");

        result.pool.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn corrupt_database_salvages_watched_folders_when_possible() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("salvage_test.db");

        // 1. Create a healthy DB, add a folder and add sync_state rows
        let result1 = init_at_path(&db_path).await.unwrap();
        assert_eq!(result1.status, DatabaseStatus::Ready);
        add_folder(&result1.pool, "C:/my_important_photos")
            .await
            .unwrap();
        for i in 0..50 {
            update_sync_state(
                &result1.pool,
                &format!("photo{}.jpg", i),
                "hash",
                42,
                100,
                "SYNCED",
                None,
            )
            .await
            .unwrap();
        }

        let sync_rootpage: i64 =
            sqlx::query_scalar("SELECT rootpage FROM sqlite_master WHERE name = 'sync_state'")
                .fetch_one(&result1.pool)
                .await
                .unwrap();

        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&result1.pool)
            .await
            .unwrap();
        result1.pool.close().await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Corrupt the sync_state table b-tree page specifically while leaving watched_folders page intact
        let mut bytes = std::fs::read(&db_path).unwrap();
        let offset = ((sync_rootpage - 1) * 4096) as usize;
        assert!(bytes.len() >= offset + 100);
        for b in &mut bytes[offset + 20..offset + 80] {
            *b = 0xFF;
        }
        std::fs::write(&db_path, bytes).unwrap();

        // 3. Now re-init at the same path. quick_check will detect corruption in sync_state!
        let result2 = init_at_path(&db_path).await.unwrap();
        match result2.status {
            DatabaseStatus::Repaired {
                salvaged_folders,
                reason,
                ..
            } => {
                assert_eq!(salvaged_folders, 1);
                assert!(!reason.is_empty());
            }
            DatabaseStatus::Ready => panic!("Expected database to be repaired"),
        }

        let folders = get_folders(&result2.pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "C:/my_important_photos");

        result2.pool.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn corrupt_database_with_wal_and_shm_quarantines_all_files() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("wal_test.db");

        // 1. Create a healthy DB
        let init_res = init_at_path(&db_path).await.unwrap();
        assert_eq!(init_res.status, DatabaseStatus::Ready);
        add_folder(&init_res.pool, "C:/photos").await.unwrap();
        init_res.pool.close().await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Ensure -wal and -shm files exist
        let wal_path = temp_dir.join("wal_test.db-wal");
        let shm_path = temp_dir.join("wal_test.db-shm");
        std::fs::write(&wal_path, b"DUMMY_RETAINED_WAL_DATA").unwrap();
        std::fs::write(&shm_path, b"DUMMY_RETAINED_SHM_DATA").unwrap();

        // 2. Corrupt db file header so it fails with SQLITE_NOTADB / CORRUPT
        let backup_path = quarantine_corrupt_db(&db_path).await.unwrap();
        assert!(backup_path.exists());
        let parent = backup_path.parent().unwrap();
        let backup_name = backup_path.file_name().unwrap().to_str().unwrap();
        assert!(parent.join(format!("{}-wal", backup_name)).exists());
        assert!(parent.join(format!("{}-shm", backup_name)).exists());

        // Verify the original wal and shm are no longer lingering
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn locked_database_is_not_quarantined() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("locked_test.db");

        // 1. Create a valid DB
        let result = init_at_path(&db_path).await.unwrap();
        assert_eq!(result.status, DatabaseStatus::Ready);
        add_folder(&result.pool, "C:/photos").await.unwrap();
        result.pool.close().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 2. Open an exclusive file lock simulating another process holding it exclusively
        #[cfg(windows)]
        let lock_file = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&db_path)
                .unwrap()
        };

        // 3. Try to init_at_path while locked
        let attempt = init_at_path(&db_path).await;
        assert!(attempt.is_err(), "Expected locked database to return error");

        // 4. Verify that NO .corrupted file was created!
        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupted"))
            .collect();
        assert!(
            entries.is_empty(),
            "Locked database must not be quarantined! Found: {:?}",
            entries
        );

        #[cfg(windows)]
        drop(lock_file);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn rapid_repairs_avoid_name_collision() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("collision_test.db");

        // Corrupt 1
        std::fs::write(&db_path, b"CORRUPT DATA 1").unwrap();
        let res1 = init_at_path(&db_path).await.unwrap();
        let path1 = match res1.status {
            DatabaseStatus::Repaired { backup_path, .. } => backup_path,
            _ => panic!("Expected repair 1"),
        };
        res1.pool.close().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Corrupt 2 immediately (within same second)
        std::fs::write(&db_path, b"CORRUPT DATA 2").unwrap();
        let wal_file = temp_dir.join("collision_test.db-wal");
        if wal_file.exists() {
            std::fs::write(&wal_file, b"CORRUPT WAL 2").unwrap();
        }
        let res2 = init_at_path(&db_path).await.unwrap();
        let path2 = match res2.status {
            DatabaseStatus::Repaired { backup_path, .. } => backup_path,
            _ => panic!("Expected repair 2"),
        };
        res2.pool.close().await;

        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());
        assert_eq!(std::fs::read(&path1).unwrap(), b"CORRUPT DATA 1");
        assert_eq!(std::fs::read(&path2).unwrap(), b"CORRUPT DATA 2");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn migration_failure_without_corruption_is_not_quarantined() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("migration_err_test.db");

        // 1. Create a healthy DB
        let result = init_at_path(&db_path).await.unwrap();
        assert_eq!(result.status, DatabaseStatus::Ready);
        add_folder(&result.pool, "C:/important_photos")
            .await
            .unwrap();
        result.pool.close().await;

        // 2. Insert a future migration entry (simulate DB was created by a newer version of the app)
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (999999, 'future_migration', CURRENT_TIMESTAMP, 1, X'1234', 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        // 3. Re-init. sqlx::migrate will return VersionMissing(999999)
        let attempt = init_at_path(&db_path).await;
        assert!(attempt.is_err(), "Expected migration error to fail init");

        // Verify that NO .corrupted file was created!
        let corrupted_files: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupted"))
            .collect();
        assert!(
            corrupted_files.is_empty(),
            "Healthy DB with version mismatch must not be quarantined! Found: {:?}",
            corrupted_files
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
