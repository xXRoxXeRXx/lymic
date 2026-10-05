use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

const DISCARD_PATH_BATCH_SIZE: usize = 500;

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

pub async fn discard_failed_sync_paths(
    pool: &SqlitePool,
    paths: &[String],
) -> Result<(), sqlx::Error> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    for table in ["sync_state", "sync_queue"] {
        for path_batch in paths.chunks(DISCARD_PATH_BATCH_SIZE) {
            let mut query = QueryBuilder::<Sqlite>::new(format!("DELETE FROM {table} WHERE "));
            if table == "sync_queue" {
                query.push("job_id = 1 AND ");
            }
            query.push("local_path IN (");
            let mut separated = query.separated(", ");
            for path in path_batch {
                separated.push_bind(path);
            }
            separated.push_unseparated(")");
            query.build().execute(&mut *tx).await?;
        }
    }

    sqlx::query(
        "UPDATE sync_jobs SET \
         total_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1), \
         success_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'SYNCED'), \
         failure_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'FAILED'), \
         status = CASE WHEN NOT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING') THEN 'IDLE' ELSE status END, \
         current_path = CASE WHEN NOT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING') THEN NULL ELSE current_path END \
         WHERE id = 1",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}
