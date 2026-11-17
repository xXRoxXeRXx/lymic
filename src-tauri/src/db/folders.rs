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
    let path = crate::media::paths::normalize_drive_letter_str(path);
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
    let path = crate::media::paths::normalize_drive_letter_str(path);
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

#[cfg(test)]
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

#[cfg(test)]
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
    let remap = FolderRemap {
        old_prefix: super::sql_folder_prefix(old_path),
        paths: crate::media::paths::PathRemapper::new(old_path, new_path),
    };

    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE watched_folders SET path = ? WHERE id = ?")
        .bind(&remap.paths.clean_new)
        .bind(folder_id)
        .execute(&mut *tx)
        .await?;

    for (table, column, row_filter) in [
        ("sync_state", "local_path", None),
        ("album_retry_remote_ids", "local_path", None),
        ("sync_jobs", "current_path", Some("id = 1")),
        ("sync_queue", "local_path", None),
        ("sync_queue", "upload_unit_id", None),
    ] {
        remap
            .remap_column(&mut tx, table, column, row_filter)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

struct FolderRemap {
    old_prefix: String,
    paths: crate::media::paths::PathRemapper,
}

impl FolderRemap {
    async fn remap_column(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        table: &str,
        column: &str,
        row_filter: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let filter_prefix = row_filter.map(|f| format!("{f} AND ")).unwrap_or_default();
        let path_cond = super::sql_path_is_or_descendant_of(column, "?1");
        let paths: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT DISTINCT {column} FROM {table} WHERE {filter_prefix}{column} IS NOT NULL AND {path_cond}"
        ))
        .bind(&self.old_prefix)
        .fetch_all(&mut **tx)
        .await?;
        for old_row_path in paths {
            if let Some(new_row_path) = self.paths.remap_if_changed(&old_row_path) {
                sqlx::query(&format!(
                    "UPDATE {table} SET {column} = ? WHERE {filter_prefix}{column} = ?"
                ))
                .bind(&new_row_path)
                .bind(&old_row_path)
                .execute(&mut **tx)
                .await?;
            }
        }
        Ok(())
    }
}
