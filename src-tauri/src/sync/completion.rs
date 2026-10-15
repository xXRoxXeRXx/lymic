use crate::{
    db,
    immich::{AlbumId, RemoteAssetId},
    media::model::HashedUploadUnit,
};
use sqlx::SqlitePool;

/// A completion which is either terminal or waits for a batched Album Assignment.
#[derive(Debug)]
pub(crate) enum UploadUnitCompletion {
    Complete(CompletionResult),
    Failed {
        completion: CompletionResult,
        reason: String,
    },
    PendingAlbum(Box<PendingAlbumUnit>),
}

#[derive(Debug)]
pub(crate) struct CompletionResult {
    queue_results: Vec<(String, String)>,
    uploaded_count: usize,
}

impl CompletionResult {
    pub(crate) fn queue_results(&self) -> &[(String, String)] {
        &self.queue_results
    }

    pub(crate) fn uploaded_count(&self) -> usize {
        self.uploaded_count
    }
}

#[cfg(test)]
impl UploadUnitCompletion {
    pub(crate) fn into_pending_album(self) -> Option<PendingAlbumUnit> {
        match self {
            Self::PendingAlbum(pending) => Some(*pending),
            Self::Complete(_) | Self::Failed { .. } => None,
        }
    }

    pub(crate) fn into_complete(self) -> Option<CompletionResult> {
        match self {
            Self::Complete(completion) => Some(completion),
            Self::PendingAlbum(_) | Self::Failed { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingAlbumUnit {
    pub album_id: AlbumId,
    pub remote_id: RemoteAssetId,
    pub unit: HashedUploadUnit,
    pub uploaded_count: usize,
}

/// Starts the single completion path after an Upload Unit's Remote Asset is known.
///
/// Every source (new upload, Duplicate, or recovered remote ID) persists that ID before
/// returning a terminal result. Target Albums defer the terminal state until their batched
/// assignment succeeds.
pub(crate) async fn begin_upload_unit_completion(
    pool: &SqlitePool,
    unit: HashedUploadUnit,
    remote_id: RemoteAssetId,
    uploaded_count: usize,
) -> Result<UploadUnitCompletion, String> {
    db::update_sync_queue_remote_id(pool, &unit.image.path, &remote_id)
        .await
        .map_err(|error| {
            format!(
                "Could not persist remote ID for '{}': {error}",
                unit.image.path
            )
        })?;

    if let Some(album_id) = unit.image.target_album_id.clone() {
        return Ok(UploadUnitCompletion::PendingAlbum(Box::new(
            PendingAlbumUnit {
                album_id,
                remote_id,
                unit,
                uploaded_count,
            },
        )));
    }

    complete_terminal_unit(pool, unit, remote_id, uploaded_count)
        .await
        .map(UploadUnitCompletion::Complete)
}

/// Finishes a Target Album unit only after Immich accepted its batched Album Assignment.
pub(crate) async fn complete_album_assignment(
    pool: &SqlitePool,
    pending: PendingAlbumUnit,
) -> Result<CompletionResult, String> {
    complete_terminal_unit(
        pool,
        pending.unit,
        pending.remote_id,
        pending.uploaded_count,
    )
    .await
}

/// Durably records an Upload Unit as failed when its successful completion cannot persist.
///
/// The queue is the recovery boundary, so a failure result is only returned once every
/// queue member has been updated. Sync-state writes are best-effort here: their original
/// failure is already what triggered this path, while the queue status prevents a false
/// synchronized result and preserves a retryable Remote Asset ID.
pub(crate) async fn fail_upload_unit(
    pool: &SqlitePool,
    unit: &HashedUploadUnit,
    reason: &str,
) -> Result<CompletionResult, String> {
    for asset in unit.assets() {
        let _ =
            db::mark_sync_failed(pool, &asset.path, asset.mtime, asset.size as i64, reason).await;
        db::update_sync_queue_failure(pool, &asset.path, reason)
            .await
            .map_err(|error| {
                format!(
                    "Could not persist failed queue state for '{}': {error}",
                    asset.path
                )
            })?;
    }

    Ok(CompletionResult {
        queue_results: unit
            .assets()
            .map(|asset| (asset.path.clone(), "FAILED".to_string()))
            .collect(),
        uploaded_count: 0,
    })
}

async fn complete_terminal_unit(
    pool: &SqlitePool,
    unit: HashedUploadUnit,
    remote_id: RemoteAssetId,
    uploaded_count: usize,
) -> Result<CompletionResult, String> {
    let states = unit
        .assets()
        .map(|asset| db::UploadedAssetState {
            path: &asset.path,
            hash: &asset
                .checksums
                .as_ref()
                .expect("hashed unit has checksums")
                .sha1_base64,
            mtime: asset.mtime,
            size: asset.size as i64,
        })
        .collect::<Vec<_>>();
    db::persist_uploaded_unit(pool, &states, &remote_id)
        .await
        .map_err(|error| {
            format!(
                "Could not persist synchronized state for '{}': {error}",
                unit.image.path
            )
        })?;

    Ok(CompletionResult {
        queue_results: unit
            .assets()
            .map(|asset| (asset.path.clone(), "SYNCED".to_string()))
            .collect(),
        uploaded_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::{self, QueueAsset},
        immich::{AlbumId, RemoteAssetId},
        media::model::{Checksums, HashedUploadUnit, SyncAsset},
    };
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    async fn test_pool() -> sqlx::SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn upload_unit(target_album_id: Option<AlbumId>) -> HashedUploadUnit {
        HashedUploadUnit {
            image: SyncAsset {
                path: "photo.jpg".to_string(),
                size: 123,
                checksums: Some(Checksums {
                    sha1_base64: "photo-hash".to_string(),
                }),
                mtime: 42,
                target_album_id,
                remote_id: None,
            },
            live_photo_video: None,
        }
    }

    async fn enqueue(pool: &sqlx::SqlitePool, target_album_id: Option<&AlbumId>) {
        db::enqueue_sync_assets(
            pool,
            &[QueueAsset {
                path: "photo.jpg",
                size: 123,
                mtime: 42,
                target_album_id,
            }],
            &[],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn completes_a_non_album_upload_unit_with_persisted_remote_id_and_terminal_result() {
        let pool = test_pool().await;
        enqueue(&pool, None).await;

        let completion = begin_upload_unit_completion(
            &pool,
            upload_unit(None),
            RemoteAssetId::new("remote-photo"),
            1,
        )
        .await
        .unwrap()
        .into_complete()
        .expect("a non-album Upload Unit must complete immediately");

        assert_eq!(
            completion.queue_results(),
            [("photo.jpg".to_string(), "SYNCED".to_string())]
        );
        assert_eq!(completion.uploaded_count(), 1);
        let queue_remote_id: Option<String> =
            sqlx::query_scalar("SELECT remote_id FROM sync_queue WHERE local_path = 'photo.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queue_remote_id.as_deref(), Some("remote-photo"));
        let state =
            sqlx::query("SELECT status, remote_id FROM sync_state WHERE local_path = 'photo.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state.get::<String, _>("status"), "SYNCED");
        assert_eq!(
            state.get::<Option<String>, _>("remote_id").as_deref(),
            Some("remote-photo")
        );
    }

    #[tokio::test]
    async fn completes_a_live_photo_upload_unit_coherently_for_both_assets() {
        let pool = test_pool().await;
        db::enqueue_upload_units(
            &pool,
            &[db::QueueUploadUnit {
                image: db::QueueUploadAsset {
                    path: "photo.jpg".to_string(),
                    size: 123,
                    mtime: 42,
                    target_album_id: None,
                },
                live_photo_video: Some(db::QueueUploadAsset {
                    path: "photo.mov".to_string(),
                    size: 456,
                    mtime: 42,
                    target_album_id: None,
                }),
            }],
            &[],
        )
        .await
        .unwrap();
        let mut unit = upload_unit(None);
        unit.live_photo_video = Some(SyncAsset {
            path: "photo.mov".to_string(),
            size: 456,
            checksums: Some(Checksums {
                sha1_base64: "video-hash".to_string(),
            }),
            mtime: 42,
            target_album_id: None,
            remote_id: None,
        });

        let completion =
            begin_upload_unit_completion(&pool, unit, RemoteAssetId::new("remote-photo"), 2)
                .await
                .unwrap()
                .into_complete()
                .unwrap();

        assert_eq!(completion.uploaded_count(), 2);
        assert_eq!(completion.queue_results().len(), 2);
        let states: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sync_state WHERE status = 'SYNCED' AND local_path IN ('photo.jpg', 'photo.mov')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(states, 2);
    }

    #[tokio::test]
    async fn completes_a_target_album_upload_unit_only_after_album_assignment() {
        let pool = test_pool().await;
        let album_id = AlbumId::new("album-1");
        enqueue(&pool, Some(&album_id)).await;

        let pending = begin_upload_unit_completion(
            &pool,
            upload_unit(Some(album_id.clone())),
            RemoteAssetId::new("remote-photo"),
            0,
        )
        .await
        .unwrap()
        .into_pending_album()
        .expect("Target Album completion must wait for assignment");

        assert!(
            sqlx::query("SELECT 1 FROM sync_state WHERE local_path = 'photo.jpg'")
                .fetch_optional(&pool)
                .await
                .unwrap()
                .is_none()
        );
        let completion = complete_album_assignment(&pool, pending).await.unwrap();
        assert_eq!(
            completion.queue_results(),
            [("photo.jpg".to_string(), "SYNCED".to_string())]
        );
        assert_eq!(completion.uploaded_count(), 0);
    }

    #[tokio::test]
    async fn refuses_to_report_a_synchronized_upload_unit_when_remote_id_cannot_persist() {
        let pool = test_pool().await;

        let error = begin_upload_unit_completion(
            &pool,
            upload_unit(None),
            RemoteAssetId::new("remote-photo"),
            1,
        )
        .await
        .expect_err("a completion without its queued persistence record must fail");

        assert!(error.contains("Could not persist remote ID"));
        assert!(
            sqlx::query("SELECT 1 FROM sync_state WHERE local_path = 'photo.jpg'")
                .fetch_optional(&pool)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn refuses_to_report_a_synchronized_upload_unit_when_terminal_state_cannot_persist() {
        let pool = test_pool().await;
        enqueue(&pool, None).await;
        sqlx::query("DROP TABLE sync_state")
            .execute(&pool)
            .await
            .unwrap();

        let error = begin_upload_unit_completion(
            &pool,
            upload_unit(None),
            RemoteAssetId::new("remote-photo"),
            1,
        )
        .await
        .expect_err("a terminal persistence failure must not produce SYNCED results");

        assert!(error.contains("Could not persist synchronized state"));
        let failed = fail_upload_unit(&pool, &upload_unit(None), &error)
            .await
            .expect("the queue must persist the failure outcome");
        assert_eq!(
            failed.queue_results(),
            [("photo.jpg".to_string(), "FAILED".to_string())]
        );
        let queue_status: String =
            sqlx::query_scalar("SELECT status FROM sync_queue WHERE local_path = 'photo.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queue_status, "FAILED");
    }
}
