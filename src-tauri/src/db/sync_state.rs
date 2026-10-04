use sqlx::{Row, SqlitePool};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedSyncEntry {
    pub local_path: String,
    pub failure_reason: String,
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
