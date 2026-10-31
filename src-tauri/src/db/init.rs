use super::folders::{
    bind_removable_folder, restore_folder, RemovableFolderBinding, WatchedFolder,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

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

#[derive(Debug)]
pub(crate) enum InitFailure {
    Recoverable(String),
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
    if let sqlx::migrate::MigrateError::VersionMismatch(version) = &err {
        return InitFailure::Recoverable(format!(
            "Migration checksum mismatch for version {} (an app update changed the database schema)",
            version
        ));
    }

    let mut current_source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(src) = current_source {
        if let Some(sqlx_err) = src.downcast_ref::<sqlx::Error>() {
            if let Some(reason) = detect_corruption(sqlx_err) {
                return InitFailure::Recoverable(reason);
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
        .foreign_keys(true)
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
            Some(reason) => InitFailure::Recoverable(reason),
            None => InitFailure::Other(Box::new(e)),
        })?;

    let status: String = row
        .try_get(0)
        .map_err(|e| InitFailure::Other(Box::new(e)))?;
    if !status.eq_ignore_ascii_case("ok") {
        return Err(InitFailure::Recoverable(format!(
            "SQLite quick_check reported corruption: {}",
            status
        )));
    }

    let mut connection = pool
        .acquire()
        .await
        .map_err(|e| InitFailure::Other(Box::new(e)))?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await
        .map_err(|e| InitFailure::Other(Box::new(e)))?;

    let migration_result = sqlx::migrate!("./migrations").run(&mut *connection).await;

    if let Err(error) = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await
    {
        let _ = connection.close().await;
        return Err(InitFailure::Other(Box::new(error)));
    }

    migration_result.map_err(classify_migrate_error)?;

    let foreign_key_violation = sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&mut *connection)
        .await
        .map_err(|e| InitFailure::Other(Box::new(e)))?;
    if foreign_key_violation.is_some() {
        return Err(InitFailure::Other(
            "SQLite foreign_key_check found a constraint violation after migration".into(),
        ));
    }

    Ok(())
}

#[derive(Debug)]
struct SalvagedFolder {
    folder: WatchedFolder,
    removable_binding: Option<RemovableFolderBinding>,
}

async fn get_watched_folders_for_salvage(
    pool: &SqlitePool,
) -> Result<Vec<WatchedFolder>, sqlx::Error> {
    if let Ok(rows) = sqlx::query(
        "SELECT id, path, recursive, target_album_id, exclusion_patterns FROM watched_folders",
    )
    .fetch_all(pool)
    .await
    {
        return rows
            .into_iter()
            .map(|row| {
                Ok(WatchedFolder {
                    id: row.try_get("id")?,
                    path: row.try_get("path")?,
                    recursive: row.try_get("recursive")?,
                    target_album_id: row.try_get("target_album_id")?,
                    exclusion_patterns: serde_json::from_str(
                        &row.try_get::<String, _>("exclusion_patterns")?,
                    )
                    .unwrap_or_default(),
                })
            })
            .collect();
    }

    if let Ok(rows) =
        sqlx::query("SELECT id, path, recursive, target_album_id FROM watched_folders")
            .fetch_all(pool)
            .await
    {
        return rows
            .into_iter()
            .map(|row| {
                Ok(WatchedFolder {
                    id: row.try_get("id")?,
                    path: row.try_get("path")?,
                    recursive: row.try_get("recursive")?,
                    target_album_id: row.try_get("target_album_id")?,
                    exclusion_patterns: Vec::new(),
                })
            })
            .collect();
    }

    sqlx::query("SELECT id, path, recursive FROM watched_folders")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(WatchedFolder {
                id: row.try_get("id")?,
                path: row.try_get("path")?,
                recursive: row.try_get("recursive")?,
                target_album_id: None,
                exclusion_patterns: Vec::new(),
            })
        })
        .collect()
}

async fn try_salvage_folders(db_path: &Path) -> Vec<SalvagedFolder> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .read_only(true)
        .foreign_keys(true)
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

    let folders = get_watched_folders_for_salvage(&pool)
        .await
        .unwrap_or_default();
    let removable_bindings = sqlx::query_as::<_, RemovableFolderBinding>(
        "SELECT watched_folder_id, volume_id, relative_path FROM removable_watched_folders",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    pool.close().await;
    folders
        .into_iter()
        .map(|folder| SalvagedFolder {
            removable_binding: removable_bindings
                .iter()
                .find(|binding| binding.watched_folder_id == folder.id)
                .cloned(),
            folder,
        })
        .collect()
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

pub(crate) async fn quarantine_unusable_db(db_path: &Path) -> Result<PathBuf, std::io::Error> {
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

async fn repair_unusable_db(
    db_path: &Path,
    reason: String,
) -> Result<DbInitResult, Box<dyn std::error::Error + Send + Sync>> {
    let salvaged = try_salvage_folders(db_path).await;
    let backup_path = quarantine_unusable_db(db_path).await?;

    let new_pool = open_pool(db_path).await?;
    match verify_and_migrate(&new_pool).await {
        Ok(()) => {}
        Err(InitFailure::Recoverable(r)) => {
            return Err(format!("Newly created database could not be used: {}", r).into())
        }
        Err(InitFailure::Other(e)) => return Err(e),
    }

    let mut restored_count = 0;
    for salvaged_folder in salvaged {
        let folder = salvaged_folder.folder;
        if let Ok(restored_folder_id) = restore_folder(
            &new_pool,
            &folder.path,
            folder.recursive,
            folder.target_album_id.as_ref(),
            &folder.exclusion_patterns,
        )
        .await
        {
            restored_count += 1;
            if let Some(binding) = salvaged_folder.removable_binding {
                let _ = bind_removable_folder(
                    &new_pool,
                    restored_folder_id,
                    &binding.volume_id,
                    &binding.relative_path,
                )
                .await;
            }
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
                Err(InitFailure::Recoverable(reason)) => {
                    pool.close().await;
                    repair_unusable_db(db_path, reason).await
                }
                Err(InitFailure::Other(err)) => {
                    pool.close().await;
                    Err(err)
                }
            },
            Err(e) => match detect_corruption(&e) {
                Some(reason) => repair_unusable_db(db_path, reason).await,
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
            Err(InitFailure::Recoverable(reason)) => {
                pool.close().await;
                repair_unusable_db(db_path, reason).await
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
