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
    let clean_new =
        normalize_windows_drive_letter(trim_trailing_separators_preserving_root(new_path));
    let norm_old = crate::media::paths::normalize_path_lexical(std::path::Path::new(old_path));

    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE watched_folders SET path = ? WHERE id = ?")
        .bind(&clean_new)
        .bind(folder_id)
        .execute(&mut *tx)
        .await?;

    for table in ["sync_state", "album_retry_remote_ids"] {
        let paths: Vec<String> = sqlx::query_scalar(&format!("SELECT local_path FROM {table}"))
            .fetch_all(&mut *tx)
            .await?;
        for old_row_path in paths {
            if let Some(new_row_path) = remap_path_if_within(&old_row_path, &norm_old, &clean_new) {
                if new_row_path != old_row_path {
                    sqlx::query(&format!(
                        "UPDATE {table} SET local_path = ? WHERE local_path = ?"
                    ))
                    .bind(&new_row_path)
                    .bind(&old_row_path)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
    }

    let queue_rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT job_id, local_path, upload_unit_id FROM sync_queue")
            .fetch_all(&mut *tx)
            .await?;
    for (job_id, old_local_path, old_upload_unit_id) in queue_rows {
        let new_local_path = remap_path_if_within(&old_local_path, &norm_old, &clean_new);
        let new_upload_unit_id = remap_path_if_within(&old_upload_unit_id, &norm_old, &clean_new);
        if new_local_path.is_some() || new_upload_unit_id.is_some() {
            let target_local = new_local_path.as_deref().unwrap_or(&old_local_path);
            let target_unit = new_upload_unit_id.as_deref().unwrap_or(&old_upload_unit_id);
            if target_local != old_local_path || target_unit != old_upload_unit_id {
                sqlx::query(
                    "UPDATE sync_queue SET local_path = ?, upload_unit_id = ? \
                     WHERE job_id = ? AND local_path = ?",
                )
                .bind(target_local)
                .bind(target_unit)
                .bind(job_id)
                .bind(&old_local_path)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    if let Some(current_path) =
        sqlx::query_scalar::<_, Option<String>>("SELECT current_path FROM sync_jobs WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?
            .flatten()
    {
        if let Some(new_current_path) = remap_path_if_within(&current_path, &norm_old, &clean_new) {
            if new_current_path != current_path {
                sqlx::query("UPDATE sync_jobs SET current_path = ? WHERE id = 1")
                    .bind(&new_current_path)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    tx.commit().await?;
    Ok(())
}

fn trim_trailing_separators_preserving_root(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        return "/";
    }
    #[cfg(windows)]
    if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' && path.len() > 2 {
        return &path[..3];
    }
    trimmed
}

fn remap_path_if_within(
    candidate: &str,
    norm_old_folder: &std::path::Path,
    clean_new: &str,
) -> Option<String> {
    let norm_candidate =
        crate::media::paths::normalize_path_lexical(std::path::Path::new(candidate));
    let relative =
        crate::media::paths::relative_path_within_lexical(&norm_candidate, norm_old_folder)?;
    if relative.as_os_str().is_empty() {
        return Some(clean_new.to_owned());
    }
    let sep = if clean_new.contains('/') && !clean_new.contains('\\') {
        "/"
    } else {
        std::path::MAIN_SEPARATOR_STR
    };
    let mut remapped = if clean_new == "/" {
        "/".to_owned()
    } else {
        clean_new.trim_end_matches(['/', '\\']).to_owned()
    };
    for component in relative.components() {
        if let std::path::Component::Normal(part) = component {
            if !remapped.ends_with(['/', '\\']) {
                remapped.push_str(sep);
            }
            remapped.push_str(&part.to_string_lossy());
        }
    }
    Some(remapped)
}

fn normalize_windows_drive_letter(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("{}{}", (bytes[0] as char).to_ascii_uppercase(), &path[1..])
    } else {
        path.to_string()
    }
}
