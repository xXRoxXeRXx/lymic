use sqlx::SqlitePool;

/// The singleton job ID used for Lymic's desktop sync queue, backed by database CHECK constraint `id = 1`.
pub const SINGLETON_SYNC_JOB_ID: i64 = 1;

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

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct QueuedAsset {
    pub local_path: String,
    pub size: i64,
    pub last_modified: i64,
    pub target_album_id: Option<crate::immich::AlbumId>,
    pub remote_id: Option<crate::immich::RemoteAssetId>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct QueueAsset<'a> {
    pub path: &'a str,
    pub size: i64,
    pub mtime: i64,
    pub target_album_id: Option<&'a crate::immich::AlbumId>,
}

/// The complete synchronization outcome which must survive queue recovery.
#[derive(Debug, Clone)]
pub struct QueueUploadAsset {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub target_album_id: Option<crate::immich::AlbumId>,
}

#[derive(Debug, Clone)]
pub struct QueueUploadUnit {
    pub image: QueueUploadAsset,
    pub live_photo_video: Option<QueueUploadAsset>,
}

/// A recovered Upload Unit. Its primary asset owns the Target Album and Remote Asset ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedUploadUnit {
    pub image: QueuedAsset,
    pub live_photo_video: Option<QueuedAsset>,
}

impl QueuedUploadUnit {
    pub(crate) fn into_upload_unit(self) -> crate::media::model::UploadUnit {
        let asset = |queued: QueuedAsset| crate::media::model::Asset {
            path: queued.local_path,
            size: queued.size as u64,
            mtime: queued.last_modified,
            target_album_id: queued.target_album_id,
            remote_id: queued.remote_id,
        };
        crate::media::model::UploadUnit {
            image: asset(self.image),
            live_photo_video: self.live_photo_video.map(asset),
        }
    }
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

#[cfg(test)]
pub async fn enqueue_sync_assets(
    pool: &SqlitePool,
    assets: &[QueueAsset<'_>],
    failures: &[(String, String)],
) -> Result<SyncSnapshot, sqlx::Error> {
    let units = assets
        .iter()
        .cloned()
        .map(|image| QueueUploadUnit {
            image: QueueUploadAsset {
                path: image.path.to_owned(),
                size: image.size,
                mtime: image.mtime,
                target_album_id: image.target_album_id.cloned(),
            },
            live_photo_video: None,
        })
        .collect::<Vec<_>>();
    enqueue_upload_units(pool, &units, failures).await
}

pub async fn enqueue_upload_units(
    pool: &SqlitePool,
    units: &[QueueUploadUnit],
    failures: &[(String, String)],
) -> Result<SyncSnapshot, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    let has_pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING')",
    )
    .fetch_one(&mut *tx)
    .await?;
    let should_reset =
        (status == "IDLE" || status == "FAILED" || (status == "RUNNING" && !has_pending))
            && (!units.is_empty() || !failures.is_empty());
    if should_reset {
        sqlx::query("UPDATE sync_jobs SET status = 'RUNNING', next_sequence = 1, total_count = 0, success_count = 0, failure_count = 0, current_path = NULL WHERE id = 1")
            .execute(&mut *tx).await?;
        sqlx::query("DELETE FROM sync_queue WHERE job_id = 1")
            .execute(&mut *tx)
            .await?;
    }
    for unit in units {
        let existing_unit_id: Option<String> = sqlx::query_scalar(
            "SELECT upload_unit_id FROM sync_queue \
             WHERE job_id = 1 AND local_path IN (?, ?) ORDER BY sequence LIMIT 1",
        )
        .bind(&unit.image.path)
        .bind(unit.live_photo_video.as_ref().map(|asset| &asset.path))
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        let unit_id = existing_unit_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        for (asset, is_primary) in std::iter::once((&unit.image, true))
            .chain(unit.live_photo_video.as_ref().map(|asset| (asset, false)))
        {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND local_path = ?)",
            )
            .bind(&asset.path)
            .fetch_one(&mut *tx)
            .await?;
            let next: i64 = sqlx::query_scalar("SELECT next_sequence FROM sync_jobs WHERE id = 1")
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query(
            "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status, target_album_id, upload_unit_id, is_primary)
             VALUES (1, ?, ?, ?, ?, 'PENDING', ?, ?, ?)
             ON CONFLICT(job_id, local_path) DO UPDATE SET
               size = excluded.size, last_modified = excluded.last_modified,
               upload_unit_id = excluded.upload_unit_id, is_primary = excluded.is_primary,
               status = CASE WHEN sync_queue.status = 'SYNCED' THEN 'SYNCED' ELSE 'PENDING' END,
               failure_reason = NULL",
        )
        .bind(next)
        .bind(&asset.path)
        .bind(asset.size)
        .bind(asset.mtime)
        .bind(asset.target_album_id.as_ref().map(|id| id.as_str()))
        .bind(&unit_id)
        .bind(is_primary)
        .execute(&mut *tx)
        .await?;
            if !exists {
                sqlx::query("UPDATE sync_jobs SET next_sequence = next_sequence + 1 WHERE id = 1")
                    .execute(&mut *tx)
                    .await?;
            }
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

#[cfg(test)]
pub async fn next_sync_queue_block(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<QueuedAsset>, sqlx::Error> {
    sqlx::query_as::<_, QueuedAsset>(
        "SELECT local_path, size, last_modified, target_album_id, remote_id FROM sync_queue
         WHERE job_id = 1 AND status = 'PENDING' ORDER BY sequence LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn next_queued_upload_units(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<QueuedUploadUnit>, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct Row {
        upload_unit_id: String,
        is_primary: bool,
        #[sqlx(flatten)]
        asset: QueuedAsset,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT COALESCE(upload_unit_id, local_path) AS upload_unit_id, is_primary, \
                local_path, size, last_modified, target_album_id, remote_id FROM sync_queue \
         WHERE job_id = 1 AND status = 'PENDING' ORDER BY sequence",
    )
    .fetch_all(pool)
    .await?;

    let mut units = Vec::new();
    let mut current_id = None;
    let mut current: Option<QueuedUploadUnit> = None;
    for row in rows {
        if current_id.as_deref() != Some(row.upload_unit_id.as_str()) {
            if let Some(unit) = current.take() {
                units.push(unit);
                if units.len() as i64 == limit {
                    break;
                }
            }
            current_id = Some(row.upload_unit_id);
            current = Some(QueuedUploadUnit {
                image: row.asset,
                live_photo_video: None,
            });
        } else if let Some(unit) = &mut current {
            if row.is_primary {
                let previous_image = std::mem::replace(&mut unit.image, row.asset);
                unit.live_photo_video = Some(previous_image);
            } else {
                unit.live_photo_video = Some(row.asset);
            }
        }
    }
    if units.len() as i64 != limit {
        if let Some(unit) = current {
            units.push(unit);
        }
    }
    Ok(units)
}

pub async fn update_sync_queue_remote_id(
    pool: &SqlitePool,
    path: &str,
    remote_id: &crate::immich::RemoteAssetId,
) -> Result<(), sqlx::Error> {
    let result =
        sqlx::query("UPDATE sync_queue SET remote_id = ? WHERE job_id = 1 AND local_path = ?")
            .bind(remote_id.as_str())
            .bind(path)
            .execute(pool)
            .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

#[cfg(test)]
pub async fn update_upload_unit_remote_id(
    pool: &SqlitePool,
    primary_path: &str,
    remote_id: &crate::immich::RemoteAssetId,
) -> Result<(), sqlx::Error> {
    update_sync_queue_remote_id(pool, primary_path, remote_id).await
}

pub async fn update_sync_queue_failure(
    pool: &SqlitePool,
    path: &str,
    failure_reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE sync_queue SET status = 'FAILED', failure_reason = ? WHERE job_id = 1 AND local_path = ?",
    )
    .bind(failure_reason)
    .bind(path)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn has_pending_sync_queue_items(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING')",
    )
    .fetch_one(pool)
    .await
}

/// Removes work that was still pending when the Watched Folder's exclusions changed.
/// Work already materialized by the processor is deliberately outside this boundary.
pub async fn discard_pending_sync_paths(
    pool: &SqlitePool,
    paths: &[String],
) -> Result<SyncSnapshot, sqlx::Error> {
    if paths.is_empty() {
        return get_sync_snapshot(pool).await;
    }

    let mut tx = pool.begin().await?;
    for path_batch in paths.chunks(500) {
        let mut query = sqlx::QueryBuilder::new(
            "DELETE FROM sync_queue WHERE job_id = 1 AND status = 'PENDING' AND local_path IN (",
        );
        let mut separated = query.separated(", ");
        for path in path_batch {
            separated.push_bind(path);
        }
        separated.push_unseparated(")");
        query.build().execute(&mut *tx).await?;
    }
    update_counts_and_snapshot(tx, true, None).await
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
    query.push("END, failure_reason = CASE WHEN status = 'SYNCED' THEN NULL ELSE failure_reason END WHERE job_id = 1 AND local_path IN (");
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
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE sync_jobs SET status = 'IDLE', current_path = NULL \
         WHERE id = 1 AND status = 'RUNNING' \
           AND NOT EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING')",
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.commit().await?;
        return Ok(None);
    }
    let snapshot = sqlx::query_as::<_, SyncSnapshot>(SNAPSHOT_SELECT)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(snapshot))
}

pub async fn reset_failed_queue_items_for_folder(
    pool: &SqlitePool,
    folder_path: &str,
) -> Result<u64, sqlx::Error> {
    let clean_path = folder_path.trim_end_matches(['/', '\\']);
    let mut tx = pool.begin().await?;

    let path_cond = super::sql_path_is_or_descendant_of("local_path", "?1");

    let queue_sql = format!(
        "UPDATE sync_queue \
         SET status = 'PENDING', failure_reason = NULL \
         WHERE job_id = {SINGLETON_SYNC_JOB_ID} AND status = 'FAILED' \
           AND {path_cond}"
    );

    let queue_res = sqlx::query(&queue_sql)
        .bind(clean_path)
        .execute(&mut *tx)
        .await?;

    let state_sql = format!(
        "UPDATE sync_state \
         SET status = 'PENDING', failure_reason = NULL \
         WHERE status = 'FAILED' \
           AND {path_cond}"
    );

    sqlx::query(&state_sql)
        .bind(clean_path)
        .execute(&mut *tx)
        .await?;

    if queue_res.rows_affected() > 0 {
        let job_sql = format!(
            "UPDATE sync_jobs SET status = 'RUNNING' WHERE id = {SINGLETON_SYNC_JOB_ID} AND status = 'IDLE'"
        );
        sqlx::query(&job_sql).execute(&mut *tx).await?;
    }

    tx.commit().await?;
    Ok(queue_res.rows_affected())
}
