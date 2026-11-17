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
    let local_path_cond = super::sql_path_is_or_descendant_of("local_path", "?1");
    let upload_unit_cond = super::sql_path_is_or_descendant_of("upload_unit_id", "?1");

    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE watched_folders SET path = ? WHERE id = ?")
        .bind(&remap.paths.clean_new)
        .bind(folder_id)
        .execute(&mut *tx)
        .await?;

    for (table, column, extra_filter) in [
        ("sync_state", "local_path", ""),
        ("album_retry_remote_ids", "local_path", ""),
        (
            "sync_jobs",
            "current_path",
            "id = 1 AND current_path IS NOT NULL AND ",
        ),
    ] {
        remap
            .remap_column(&mut tx, table, column, extra_filter)
            .await?;
    }

    let queue_rows: Vec<(i64, String, String)> = sqlx::query_as(&format!(
        "SELECT job_id, local_path, upload_unit_id FROM sync_queue WHERE ({local_path_cond} OR {upload_unit_cond})"
    ))
    .bind(&remap.old_prefix)
    .fetch_all(&mut *tx)
    .await?;
    for (job_id, old_local_path, old_upload_unit_id) in queue_rows {
        let new_local = remap.paths.remap_if_changed(&old_local_path);
        let new_unit = remap.paths.remap_if_changed(&old_upload_unit_id);
        if new_local.is_some() || new_unit.is_some() {
            sqlx::query(
                "UPDATE sync_queue SET local_path = ?, upload_unit_id = ? \
                 WHERE job_id = ? AND local_path = ?",
            )
            .bind(new_local.as_deref().unwrap_or(&old_local_path))
            .bind(new_unit.as_deref().unwrap_or(&old_upload_unit_id))
            .bind(job_id)
            .bind(&old_local_path)
            .execute(&mut *tx)
            .await?;
        }
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
        extra_filter: &str,
    ) -> Result<(), sqlx::Error> {
        let path_cond = super::sql_path_is_or_descendant_of(column, "?1");
        let paths: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT {column} FROM {table} WHERE {extra_filter}{path_cond}"
        ))
        .bind(&self.old_prefix)
        .fetch_all(&mut **tx)
        .await?;
        let row_filter = if table == "sync_jobs" {
            "id = 1 AND "
        } else {
            ""
        };
        for old_row_path in paths {
            if let Some(new_row_path) = self.paths.remap_if_changed(&old_row_path) {
                sqlx::query(&format!(
                    "UPDATE {table} SET {column} = ? WHERE {row_filter}{column} = ?"
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

fn normalize_windows_drive_letter(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("{}{}", (bytes[0] as char).to_ascii_uppercase(), &path[1..])
    } else {
        path.to_string()
    }
}
