use crate::immich::AlbumId;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, serde::Serialize)]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub recursive: bool,
    pub target_album_id: Option<AlbumId>,
    pub exclusion_patterns: Vec<String>,
}

pub async fn add_folder(
    pool: &SqlitePool,
    path: &str,
    target_album_id: Option<&AlbumId>,
) -> Result<i64, sqlx::Error> {
    let path = normalize_windows_drive_letter(path);
    let result = sqlx::query("INSERT INTO watched_folders (path, target_album_id) VALUES (?, ?)")
        .bind(path)
        .bind(target_album_id.map(|id| id.as_str()))
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}

pub async fn restore_folder(
    pool: &SqlitePool,
    path: &str,
    recursive: bool,
    target_album_id: Option<&AlbumId>,
    exclusion_patterns: &[String],
) -> Result<i64, sqlx::Error> {
    let path = normalize_windows_drive_letter(path);
    let exclusion_patterns =
        serde_json::to_string(exclusion_patterns).expect("exclusion patterns serialize");
    let result = sqlx::query(
        "INSERT OR IGNORE INTO watched_folders \
         (path, recursive, target_album_id, exclusion_patterns) VALUES (?, ?, ?, ?)",
    )
    .bind(path)
    .bind(recursive)
    .bind(target_album_id.map(|id| id.as_str()))
    .bind(exclusion_patterns)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_folders(pool: &SqlitePool) -> Result<Vec<WatchedFolder>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, path, recursive, target_album_id, exclusion_patterns FROM watched_folders",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
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
        .collect()
}

pub async fn update_exclusion_patterns(
    pool: &SqlitePool,
    id: i64,
    exclusion_patterns: &[String],
) -> Result<bool, sqlx::Error> {
    let value = serde_json::to_string(exclusion_patterns).expect("exclusion patterns serialize");
    Ok(
        sqlx::query("UPDATE watched_folders SET exclusion_patterns = ? WHERE id = ?")
            .bind(value)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
            == 1,
    )
}

pub async fn remove_folder(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watched_folders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[allow(dead_code)]
pub struct RemovableFolderBinding {
    pub watched_folder_id: i64,
    pub volume_id: String,
    pub relative_path: String,
}

pub async fn bind_removable_folder(
    pool: &SqlitePool,
    watched_folder_id: i64,
    volume_id: &str,
    relative_path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO removable_watched_folders (watched_folder_id, volume_id, relative_path) \
         VALUES (?, ?, ?) \
         ON CONFLICT(watched_folder_id) DO UPDATE SET volume_id = excluded.volume_id, relative_path = excluded.relative_path",
    )
    .bind(watched_folder_id)
    .bind(volume_id)
    .bind(relative_path)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_removable_folder_binding(
    pool: &SqlitePool,
    watched_folder_id: i64,
) -> Result<Option<RemovableFolderBinding>, sqlx::Error> {
    sqlx::query_as::<_, RemovableFolderBinding>(
        "SELECT watched_folder_id, volume_id, relative_path FROM removable_watched_folders WHERE watched_folder_id = ?",
    )
    .bind(watched_folder_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_removable_folders_by_volume(
    pool: &SqlitePool,
    volume_id: &str,
) -> Result<Vec<RemovableFolderBinding>, sqlx::Error> {
    sqlx::query_as::<_, RemovableFolderBinding>(
        "SELECT watched_folder_id, volume_id, relative_path FROM removable_watched_folders WHERE volume_id = ?",
    )
    .bind(volume_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn unbind_removable_folder(
    pool: &SqlitePool,
    watched_folder_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM removable_watched_folders WHERE watched_folder_id = ?")
        .bind(watched_folder_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remap_watched_folder_path(
    pool: &SqlitePool,
    folder_id: i64,
    old_path: &str,
    new_path: &str,
) -> Result<(), sqlx::Error> {
    let clean_old = normalize_windows_drive_letter(old_path.trim_end_matches(['/', '\\']));
    let clean_new = normalize_windows_drive_letter(new_path.trim_end_matches(['/', '\\']));

    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE watched_folders SET path = ? WHERE id = ?")
        .bind(&clean_new)
        .bind(folder_id)
        .execute(&mut *tx)
        .await?;

    let local_path_cond = super::sql_path_is_or_descendant_of("local_path", "?1");
    let upload_unit_cond = super::sql_path_is_or_descendant_of("upload_unit_id", "?1");
    let current_path_cond = super::sql_path_is_or_descendant_of("current_path", "?1");

    let sync_state_sql = format!(
        "UPDATE sync_state \
         SET local_path = ?2 || substr(local_path, length(?1) + 1) \
         WHERE {local_path_cond}"
    );
    sqlx::query(&sync_state_sql)
        .bind(&clean_old)
        .bind(&clean_new)
        .execute(&mut *tx)
        .await?;

    let sync_queue_path_sql = format!(
        "UPDATE sync_queue \
         SET local_path = ?2 || substr(local_path, length(?1) + 1) \
         WHERE {local_path_cond}"
    );
    sqlx::query(&sync_queue_path_sql)
        .bind(&clean_old)
        .bind(&clean_new)
        .execute(&mut *tx)
        .await?;

    let sync_queue_unit_sql = format!(
        "UPDATE sync_queue \
         SET upload_unit_id = ?2 || substr(upload_unit_id, length(?1) + 1) \
         WHERE {upload_unit_cond}"
    );
    sqlx::query(&sync_queue_unit_sql)
        .bind(&clean_old)
        .bind(&clean_new)
        .execute(&mut *tx)
        .await?;

    let sync_jobs_sql = format!(
        "UPDATE sync_jobs \
         SET current_path = ?2 || substr(current_path, length(?1) + 1) \
         WHERE {current_path_cond}"
    );
    sqlx::query(&sync_jobs_sql)
        .bind(&clean_old)
        .bind(&clean_new)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

fn normalize_windows_drive_letter(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("{}{}", (bytes[0] as char).to_ascii_uppercase(), &path[1..])
    } else {
        path.to_string()
    }
}
