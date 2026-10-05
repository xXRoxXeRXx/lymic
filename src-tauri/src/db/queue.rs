use sqlx::SqlitePool;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SyncSnapshot {
    pub status: String,
    pub total: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub current_path: Option<String>,
    pub total_bytes: i64,
    pub completed_bytes: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueuedAsset {
    pub local_path: String,
    pub size: i64,
    pub last_modified: i64,
}

#[derive(Debug, Clone)]
pub struct QueueAsset<'a> {
    pub path: &'a str,
    pub size: i64,
    pub mtime: i64,
}

const SNAPSHOT_SELECT: &str = "SELECT status, total_count AS total, success_count AS succeeded, failure_count AS failed, current_path, \
         COALESCE((SELECT SUM(size) FROM sync_queue WHERE job_id = 1), 0) AS total_bytes, \
         COALESCE((SELECT SUM(size) FROM sync_queue WHERE job_id = 1 AND status IN ('SYNCED', 'FAILED')), 0) AS completed_bytes \
  FROM sync_jobs WHERE id = 1";

pub async fn recover_sync_job(pool: &SqlitePool) -> Result<SyncSnapshot, sqlx::Error> {
    // A process cannot safely know whether an in-flight HTTP upload completed. Recovery
    // therefore always requires explicit user confirmation before processing resumes.
    sqlx::query("UPDATE sync_jobs SET status = 'PAUSED' WHERE id = 1 AND status = 'RUNNING'")
        .execute(pool)
        .await?;
    get_sync_snapshot(pool).await
}

pub async fn get_sync_snapshot(pool: &SqlitePool) -> Result<SyncSnapshot, sqlx::Error> {
    sqlx::query_as::<_, SyncSnapshot>(SNAPSHOT_SELECT)
        .fetch_one(pool)
        .await
}

pub async fn enqueue_sync_assets(
    pool: &SqlitePool,
    assets: &[QueueAsset<'_>],
    failures: &[(String, String)],
) -> Result<SyncSnapshot, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    if status == "IDLE" && (!assets.is_empty() || !failures.is_empty()) {
        sqlx::query("UPDATE sync_jobs SET status = 'RUNNING', next_sequence = 1, total_count = 0, success_count = 0, failure_count = 0, current_path = NULL WHERE id = 1")
            .execute(&mut *tx).await?;
        sqlx::query("DELETE FROM sync_queue WHERE job_id = 1")
            .execute(&mut *tx)
            .await?;
    }
    for asset in assets {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND local_path = ?)",
        )
        .bind(asset.path)
        .fetch_one(&mut *tx)
        .await?;
        let next: i64 = sqlx::query_scalar("SELECT next_sequence FROM sync_jobs WHERE id = 1")
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status)
             VALUES (1, ?, ?, ?, ?, 'PENDING')
             ON CONFLICT(job_id, local_path) DO UPDATE SET
               size = excluded.size, last_modified = excluded.last_modified,
               status = CASE WHEN sync_queue.status = 'SYNCED' THEN 'SYNCED' ELSE 'PENDING' END,
               failure_reason = NULL",
        )
        .bind(next)
        .bind(asset.path)
        .bind(asset.size)
        .bind(asset.mtime)
        .execute(&mut *tx)
        .await?;
        if !exists {
            sqlx::query("UPDATE sync_jobs SET next_sequence = next_sequence + 1 WHERE id = 1")
                .execute(&mut *tx)
                .await?;
        }
    }
    for (path, reason) in failures {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND local_path = ?)",
        )
        .bind(path)
        .fetch_one(&mut *tx)
        .await?;
        let next: i64 = sqlx::query_scalar("SELECT next_sequence FROM sync_jobs WHERE id = 1")
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status, failure_reason)
             VALUES (1, ?, ?, 0, 0, 'FAILED', ?)
             ON CONFLICT(job_id, local_path) DO UPDATE SET status = 'FAILED', failure_reason = excluded.failure_reason",
        ).bind(next).bind(path).bind(reason).execute(&mut *tx).await?;
        if !exists {
            sqlx::query("UPDATE sync_jobs SET next_sequence = next_sequence + 1 WHERE id = 1")
                .execute(&mut *tx)
                .await?;
        }
    }
    update_counts_and_snapshot(tx, true, None).await
}

pub async fn next_sync_queue_block(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<QueuedAsset>, sqlx::Error> {
    sqlx::query_as::<_, QueuedAsset>(
        "SELECT local_path, size, last_modified FROM sync_queue
         WHERE job_id = 1 AND status = 'PENDING' ORDER BY sequence LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn set_sync_status(pool: &SqlitePool, status: &str) -> Result<SyncSnapshot, sqlx::Error> {
    sqlx::query("UPDATE sync_jobs SET status = ?, current_path = CASE WHEN ? = 'IDLE' THEN NULL ELSE current_path END WHERE id = 1")
        .bind(status).bind(status).execute(pool).await?;
    get_sync_snapshot(pool).await
}

pub async fn finalize_queued_block(
    pool: &SqlitePool,
    results: &[(String, String)],
) -> Result<SyncSnapshot, sqlx::Error> {
    debug_assert!(!results.is_empty());
    let mut tx = pool.begin().await?;
    let mut query = sqlx::QueryBuilder::new("UPDATE sync_queue SET status = CASE local_path ");
    for (path, status) in results {
        query
            .push("WHEN ")
            .push_bind(path)
            .push(" THEN ")
            .push_bind(status)
            .push(' ');
    }
    query.push("END, failure_reason = NULL WHERE job_id = 1 AND local_path IN (");
    {
        let mut separated = query.separated(", ");
        for (path, _) in results {
            separated.push_bind(path);
        }
    }
    query.push(')');
    query.build().execute(&mut *tx).await?;
    let current_path = &results.last().expect("non-empty results").0;
    update_counts_and_snapshot(tx, false, Some(current_path)).await
}

async fn update_counts_and_snapshot(
    mut tx: sqlx::Transaction<'_, sqlx::Sqlite>,
    update_total: bool,
    current_path: Option<&str>,
) -> Result<SyncSnapshot, sqlx::Error> {
    if update_total {
        sqlx::query(
            "UPDATE sync_jobs SET \
             total_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1), \
             success_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'SYNCED'), \
             failure_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'FAILED') \
             WHERE id = 1",
        )
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE sync_jobs SET \
             success_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'SYNCED'), \
             failure_count = (SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'FAILED'), \
             current_path = ? \
             WHERE id = 1",
        )
        .bind(current_path)
        .execute(&mut *tx)
        .await?;
    }
    let snapshot = sqlx::query_as::<_, SyncSnapshot>(SNAPSHOT_SELECT)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(snapshot)
}

pub async fn complete_sync_job_if_finished(
    pool: &SqlitePool,
) -> Result<Option<SyncSnapshot>, sqlx::Error> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_queue WHERE job_id = 1 AND status = 'PENDING'",
    )
    .fetch_one(pool)
    .await?;
    if pending != 0 {
        return Ok(None);
    }
    Some(set_sync_status(pool, "IDLE").await).transpose()
}
