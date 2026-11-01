//! Hashing, bulk-check, and upload processor implementation.

use crate::{
    app::events::log_to_ui_file_error,
    audit::{self, audit_event, upload_details, SyncAuditContext},
    db,
    immich::{ImmichClient, SidecarDirectoryCache},
    media::{
        model::{HashedUploadUnit, UploadUnit},
        scanner::{calculate_stable_checksums, reuse_cached_checksums},
    },
    sync::{
        album_batch,
        completion::{self, PendingAlbumUnit, UploadUnitCompletion},
        retry::check_assets_exist_with_backoff,
        JobByteProgress, ProcessingContext, SyncCoordinatorInner,
    },
};
use futures::{future, stream::FuturesUnordered, StreamExt};
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSummary {
    pub(crate) processed: usize,
    pub(crate) uploaded: usize,
    pub(crate) failed: usize,
    #[serde(skip)]
    pub(crate) queue_results: Vec<(String, String)>,
}

pub(crate) const BULK_CHECK_CHUNK_SIZE: usize = 100;

enum BulkCheckOutcome {
    Existing(Vec<crate::immich::BulkCheckDuplicate>),
    Cancelled,
}

struct ChunkPartition {
    upload_units: Vec<HashedUploadUnit>,
    pending_album_units: Vec<PendingAlbumUnit>,
    cancelled: bool,
}

/// Processes bounded queue blocks for one synchronization run.
///
/// The run owns the collaborators that must stay consistent across blocks, while
/// `process` remains the sole block-level entry point for the queue runner.
pub(crate) struct RunProcessor {
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    client: Arc<ImmichClient>,
    audit_context: SyncAuditContext,
    progress: JobByteProgress,
    coordinator: Arc<SyncCoordinatorInner>,
    sidecar_cache: SidecarDirectoryCache,
    upload_parallelism: usize,
}

impl RunProcessor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new(
        app: tauri::AppHandle,
        pool: sqlx::SqlitePool,
        client: Arc<ImmichClient>,
        audit_context: SyncAuditContext,
        progress: JobByteProgress,
        coordinator: Arc<SyncCoordinatorInner>,
        sidecar_cache: SidecarDirectoryCache,
    ) -> Result<Self, String> {
        let upload_parallelism = load_upload_parallelism(&pool).await?;
        Ok(Self {
            app,
            pool,
            client,
            audit_context,
            progress,
            coordinator,
            sidecar_cache,
            upload_parallelism,
        })
    }

    pub(crate) async fn process(
        &self,
        units: Vec<UploadUnit>,
        active_work: ProcessingContext,
    ) -> Result<SyncSummary, String> {
        wait_while_paused(&self.coordinator).await;
        let (hashed_units, mut queue_results) = self.hash_units(units, &active_work).await;

        let mut uploaded = 0;
        for chunk in hashed_units.chunks(BULK_CHECK_CHUNK_SIZE) {
            if !can_start_work(&self.coordinator) {
                break;
            }

            let ChunkPartition {
                upload_units,
                mut pending_album_units,
                cancelled,
            } = self
                .check_and_partition_chunk(chunk, &active_work, &mut queue_results, &mut uploaded)
                .await?;

            if cancelled {
                self.flush_pending_album_units(
                    pending_album_units,
                    &mut queue_results,
                    &mut uploaded,
                )
                .await?;
                continue;
            }

            self.upload_missing_units(
                upload_units,
                &active_work,
                &mut pending_album_units,
                &mut queue_results,
                &mut uploaded,
            )
            .await?;

            self.flush_pending_album_units(pending_album_units, &mut queue_results, &mut uploaded)
                .await?;

            if !can_start_work(&self.coordinator) {
                break;
            }
        }

        Ok(summarize_block(queue_results, uploaded))
    }

    async fn hash_units(
        &self,
        units: Vec<UploadUnit>,
        active_work: &ProcessingContext,
    ) -> (Vec<HashedUploadUnit>, Vec<(String, String)>) {
        let mut queue_results = Vec::new();
        let mut hashed_units = Vec::new();
        let mut hashing = FuturesUnordered::new();
        for unit in units {
            if !can_start_work(&self.coordinator) {
                break;
            }
            let unit_work = active_work.for_upload_unit(&unit);
            if unit_work.is_cancelled() {
                continue;
            }
            hashing.push(hash_unit_or_cancellation(
                unit,
                self.pool.clone(),
                self.app.clone(),
                self.progress.clone(),
                self.audit_context.clone(),
                unit_work.cancellation(),
            ));
            if hashing.len() == 4 {
                if let Some(outcome) = hashing.next().await {
                    Self::record_hash_outcome(outcome, &mut hashed_units, &mut queue_results);
                }
            }
        }
        while let Some(outcome) = hashing.next().await {
            Self::record_hash_outcome(outcome, &mut hashed_units, &mut queue_results);
        }
        (hashed_units, queue_results)
    }

    fn record_hash_outcome(
        outcome: HashOutcome,
        hashed_units: &mut Vec<HashedUploadUnit>,
        queue_results: &mut Vec<(String, String)>,
    ) {
        queue_results.extend(outcome.queue_results);
        if let Some(unit) = outcome.unit {
            hashed_units.push(unit);
        }
    }

    async fn check_and_partition_chunk(
        &self,
        chunk: &[HashedUploadUnit],
        active_work: &ProcessingContext,
        queue_results: &mut Vec<(String, String)>,
        uploaded: &mut usize,
    ) -> Result<ChunkPartition, String> {
        let mut pending_album_units = Vec::new();
        let mut units_to_check = Vec::new();
        for unit in chunk {
            if active_work.for_hashed_upload_unit(unit).is_cancelled() {
                continue;
            }
            if let Some(remote_id) = &unit.image.remote_id {
                self.complete_resolved_unit(
                    unit.clone(),
                    remote_id.clone(),
                    &mut pending_album_units,
                    queue_results,
                    uploaded,
                )
                .await?;
            } else {
                units_to_check.push(unit.clone());
            }
        }

        let existing = match self.bulk_check_units(&units_to_check, active_work).await? {
            BulkCheckOutcome::Existing(existing) => existing,
            BulkCheckOutcome::Cancelled => {
                return Ok(ChunkPartition {
                    upload_units: Vec::new(),
                    pending_album_units,
                    cancelled: true,
                });
            }
        };
        if !can_start_work(&self.coordinator) {
            return Ok(ChunkPartition {
                upload_units: Vec::new(),
                pending_album_units,
                cancelled: false,
            });
        }

        let mut upload_units = Vec::new();
        for unit in units_to_check {
            if active_work.for_hashed_upload_unit(&unit).is_cancelled() {
                continue;
            }
            let image_hash = &unit.image.checksums.as_ref().unwrap().sha1_base64;
            if existing
                .iter()
                .any(|duplicate| &duplicate.hash == image_hash)
            {
                match album_batch::resolve_duplicate_remote_id(&self.client, &unit, &existing).await
                {
                    Ok(remote_id) => {
                        self.complete_resolved_unit(
                            unit,
                            remote_id,
                            &mut pending_album_units,
                            queue_results,
                            uploaded,
                        )
                        .await?;
                    }
                    Err(error_msg) => {
                        self.fail_duplicate_resolution(unit, error_msg, queue_results)
                            .await?;
                    }
                }
            } else {
                upload_units.push(unit);
            }
        }
        Ok(ChunkPartition {
            upload_units,
            pending_album_units,
            cancelled: false,
        })
    }

    async fn bulk_check_units(
        &self,
        units: &[HashedUploadUnit],
        active_work: &ProcessingContext,
    ) -> Result<BulkCheckOutcome, String> {
        if units.is_empty() {
            return Ok(BulkCheckOutcome::Existing(Vec::new()));
        }
        let hashes = units
            .iter()
            .flat_map(|unit| unit.assets())
            .filter_map(|asset| {
                asset
                    .checksums
                    .as_ref()
                    .map(|checksums| checksums.sha1_base64.clone())
            })
            .collect();
        let cancellations: Vec<CancellationToken> = units
            .iter()
            .map(|unit| active_work.for_hashed_upload_unit(unit).cancellation())
            .collect();
        let bulk_check =
            check_assets_exist_with_backoff(&self.client, hashes, &self.app, &self.audit_context);
        if cancellations.is_empty() {
            return bulk_check.await.map(BulkCheckOutcome::Existing);
        }
        tokio::select! {
            result = bulk_check => result.map(BulkCheckOutcome::Existing),
            _ = wait_for_all_cancellations(cancellations) => Ok(BulkCheckOutcome::Cancelled),
        }
    }

    async fn complete_resolved_unit(
        &self,
        unit: HashedUploadUnit,
        remote_id: crate::immich::RemoteAssetId,
        pending_album_units: &mut Vec<PendingAlbumUnit>,
        queue_results: &mut Vec<(String, String)>,
        uploaded: &mut usize,
    ) -> Result<(), String> {
        match begin_upload_unit_completion_or_fail(
            &self.pool,
            &self.app,
            &self.audit_context,
            unit.clone(),
            remote_id,
            0,
        )
        .await?
        {
            UploadUnitCompletion::PendingAlbum(pending) => pending_album_units.push(*pending),
            UploadUnitCompletion::Complete(completion) => {
                queue_results.extend_from_slice(completion.queue_results());
                *uploaded += completion.uploaded_count();
            }
            UploadUnitCompletion::Failed { completion, .. } => {
                queue_results.extend_from_slice(completion.queue_results());
            }
        }
        self.progress.advance(&self.app, unit.byte_size());
        Ok(())
    }

    async fn fail_duplicate_resolution(
        &self,
        unit: HashedUploadUnit,
        error_msg: String,
        queue_results: &mut Vec<(String, String)>,
    ) -> Result<(), String> {
        log_to_ui_file_error(
            &self.app,
            "ERROR",
            &error_msg,
            "log_duplicate_resolve_failed",
            &unit.image.path,
            &error_msg,
        );
        audit_event(
            &self.app,
            self.audit_context.event(
                "album.duplicate.resolution.failed",
                audit::Outcome::Failure,
                audit::Severity::Error,
                "Failed to resolve duplicate asset remote ID for target album",
                json!({
                    "local_path": unit.image.path,
                    "failure_reason": self.audit_context.safe_error(&error_msg),
                }),
            ),
        );
        let failure = completion::fail_upload_unit(&self.pool, &unit, &error_msg).await?;
        queue_results.extend_from_slice(failure.queue_results());
        self.progress.advance(&self.app, unit.byte_size());
        Ok(())
    }

    async fn upload_missing_units(
        &self,
        upload_units: Vec<HashedUploadUnit>,
        active_work: &ProcessingContext,
        pending_album_units: &mut Vec<PendingAlbumUnit>,
        queue_results: &mut Vec<(String, String)>,
        uploaded: &mut usize,
    ) -> Result<(), String> {
        let mut pending = upload_units.into_iter();
        let mut uploads = FuturesUnordered::new();
        while can_start_work(&self.coordinator) && uploads.len() < self.upload_parallelism {
            let Some((unit, cancellation)) = next_active_upload_unit(&mut pending, active_work)
            else {
                break;
            };
            uploads.push(upload_unit(
                unit,
                self.client.clone(),
                self.pool.clone(),
                self.app.clone(),
                self.progress.clone(),
                self.audit_context.clone(),
                cancellation,
                self.sidecar_cache.clone(),
            ));
        }
        while let Some(outcome) = uploads.next().await {
            let outcome = outcome?;
            *uploaded += outcome.uploaded;
            queue_results.extend(outcome.queue_results);
            if let Some(pending_album) = outcome.pending_album_unit {
                pending_album_units.push(pending_album);
            }
            if can_start_work(&self.coordinator) {
                while can_start_work(&self.coordinator) && uploads.len() < self.upload_parallelism {
                    let Some((unit, cancellation)) =
                        next_active_upload_unit(&mut pending, active_work)
                    else {
                        break;
                    };
                    uploads.push(upload_unit(
                        unit,
                        self.client.clone(),
                        self.pool.clone(),
                        self.app.clone(),
                        self.progress.clone(),
                        self.audit_context.clone(),
                        cancellation,
                        self.sidecar_cache.clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    async fn flush_pending_album_units(
        &self,
        pending_album_units: Vec<PendingAlbumUnit>,
        queue_results: &mut Vec<(String, String)>,
        uploaded: &mut usize,
    ) -> Result<(), String> {
        if pending_album_units.is_empty() || !can_start_work(&self.coordinator) {
            return Ok(());
        }
        album_batch::assign_pending_album_units(
            &self.client,
            &self.pool,
            &self.app,
            &self.audit_context,
            pending_album_units,
            queue_results,
            uploaded,
        )
        .await
    }
}

fn summarize_block(queue_results: Vec<(String, String)>, uploaded: usize) -> SyncSummary {
    let failed = queue_results
        .iter()
        .filter(|(_, status)| status == "FAILED")
        .count();
    let processed = queue_results.len();
    SyncSummary {
        processed,
        uploaded,
        failed,
        queue_results,
    }
}

async fn wait_for_all_cancellations(cancellations: Vec<CancellationToken>) {
    future::join_all(
        cancellations
            .into_iter()
            .map(|cancellation| async move { cancellation.cancelled().await }),
    )
    .await;
}

fn next_active_upload_unit(
    pending: &mut impl Iterator<Item = HashedUploadUnit>,
    active_work: &ProcessingContext,
) -> Option<(HashedUploadUnit, CancellationToken)> {
    pending.find_map(|unit| {
        let unit_work = active_work.for_hashed_upload_unit(&unit);
        (!unit_work.is_cancelled()).then(|| (unit, unit_work.cancellation()))
    })
}

fn can_start_work(coordinator: &SyncCoordinatorInner) -> bool {
    !coordinator.paused.load(Ordering::SeqCst)
}

async fn wait_while_paused(coordinator: &SyncCoordinatorInner) {
    while coordinator.paused.load(Ordering::SeqCst) {
        let notified = coordinator.resume.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !coordinator.paused.load(Ordering::SeqCst) {
            break;
        }
        notified.await;
    }
}

struct HashOutcome {
    unit: Option<HashedUploadUnit>,
    queue_results: Vec<(String, String)>,
}

async fn hash_unit_or_cancellation(
    unit: UploadUnit,
    pool: sqlx::SqlitePool,
    app: tauri::AppHandle,
    progress: JobByteProgress,
    audit_context: SyncAuditContext,
    cancellation: CancellationToken,
) -> HashOutcome {
    await_or_cancellation(
        cancellation,
        hash_unit(unit, pool, app, progress, audit_context),
    )
    .await
    .unwrap_or(HashOutcome {
        unit: None,
        queue_results: Vec::new(),
    })
}

async fn hash_unit(
    unit: UploadUnit,
    pool: sqlx::SqlitePool,
    app: tauri::AppHandle,
    progress: JobByteProgress,
    audit_context: SyncAuditContext,
) -> HashOutcome {
    let result: Result<HashedUploadUnit, String> = async {
        let mut hashed = Vec::new();
        for asset in unit.assets() {
            let cached = db::get_cached_hash(&pool, &asset.path, asset.mtime, asset.size as i64)
                .await
                .map_err(|error| {
                    format!("Could not load cached hash for {}: {}", asset.path, error)
                })?;
            if let Some(cached_sha1_base64) = cached.as_ref() {
                let path = asset.path.clone();
                let expected_size = asset.size;
                let expected_mtime = asset.mtime;
                let cached_sha1_base64 = cached_sha1_base64.clone();
                if let Some((checksums, size, mtime)) = tokio::task::spawn_blocking(move || {
                    reuse_cached_checksums(
                        &path,
                        expected_size,
                        expected_mtime,
                        &cached_sha1_base64,
                    )
                })
                .await
                .map_err(|error| format!("Hashing task failed: {}", error))?
                .map_err(|error| format!("Hashing failed: {}", error))?
                {
                    hashed.push(crate::media::model::SyncAsset {
                        path: asset.path.clone(),
                        size,
                        mtime,
                        checksums: Some(checksums),
                        target_album_id: asset.target_album_id.clone(),
                        remote_id: asset.remote_id.clone(),
                    });
                    continue;
                }
            }
            let path = asset.path.clone();
            let (checksums, size, mtime) =
                tokio::task::spawn_blocking(move || calculate_stable_checksums(&path))
                    .await
                    .map_err(|error| format!("Hashing task failed: {}", error))?
                    .map_err(|error| format!("Hashing failed: {}", error))?;
            if cached.as_deref() != Some(&checksums.sha1_base64) {
                db::update_sync_state(
                    &pool,
                    &asset.path,
                    &checksums.sha1_base64,
                    mtime,
                    size as i64,
                    "PENDING",
                    None,
                )
                .await
                .map_err(|error| {
                    format!(
                        "Could not persist pending sync state for {}: {}",
                        asset.path, error
                    )
                })?;
            }
            hashed.push(crate::media::model::SyncAsset {
                path: asset.path.clone(),
                size,
                mtime,
                checksums: Some(checksums),
                target_album_id: asset.target_album_id.clone(),
                remote_id: asset.remote_id.clone(),
            });
        }
        Ok(HashedUploadUnit {
            image: hashed.remove(0),
            live_photo_video: hashed.pop(),
        })
    }
    .await;

    match result {
        Ok(unit) => HashOutcome {
            unit: Some(unit),
            queue_results: Vec::new(),
        },
        Err(error) => {
            let paths: Vec<_> = unit.assets().map(|asset| asset.path.clone()).collect();
            for asset in unit.assets() {
                let _ = db::mark_sync_failed(
                    &pool,
                    &asset.path,
                    asset.mtime,
                    asset.size as i64,
                    &error,
                )
                .await;
            }
            let path_list = paths.join(", ");
            log_to_ui_file_error(
                &app,
                "ERROR",
                &format!("File hashing failed for '{}': {}", path_list, error),
                "log_file_hash_failed",
                &path_list,
                &error,
            );
            audit_event(&app, audit_context.event("file.hash.failed", audit::Outcome::Failure, audit::Severity::Error, "File hashing failed", json!({ "local_paths": paths, "failure_reason": audit_context.safe_error(&error) })));
            progress.advance(&app, unit.assets().map(|asset| asset.size).sum());
            HashOutcome {
                unit: None,
                queue_results: unit
                    .assets()
                    .map(|asset| (asset.path.clone(), "FAILED".to_string()))
                    .collect(),
            }
        }
    }
}

struct UploadOutcome {
    uploaded: usize,
    queue_results: Vec<(String, String)>,
    pending_album_unit: Option<PendingAlbumUnit>,
}

#[allow(clippy::too_many_arguments)]
async fn upload_unit(
    unit: HashedUploadUnit,
    client: Arc<ImmichClient>,
    pool: sqlx::SqlitePool,
    app: tauri::AppHandle,
    progress: JobByteProgress,
    audit_context: SyncAuditContext,
    cancellation: CancellationToken,
    sidecar_cache: SidecarDirectoryCache,
) -> Result<UploadOutcome, String> {
    let image_hash = unit.image.checksums.as_ref().unwrap().sha1_base64.clone();
    let video_path = unit
        .live_photo_video
        .as_ref()
        .map(|asset| asset.path.as_str());
    let operation_id = audit::AuditEvent::operation_id();
    let started = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let mut details = upload_details(
        &unit.image,
        unit.image.checksums.as_ref().unwrap(),
        &operation_id,
        &started,
        None,
        None,
        None,
    );
    if let Some(video) = &unit.live_photo_video {
        details["live_photo_local_path"] = json!(video.path);
    }
    audit_event(
        &app,
        audit_context.event(
            "file.upload.started",
            audit::Outcome::Started,
            audit::Severity::Info,
            "Upload started",
            details,
        ),
    );
    let _ = app.emit("sync-progress", &unit.image.path);

    let upload = client.upload_asset_with_live_photo(
        &unit.image.path,
        &image_hash,
        video_path,
        Some(progress.transfer_stats()),
        sidecar_cache,
    );
    let Some(upload_result) = await_or_cancellation(cancellation, upload).await else {
        return Ok(UploadOutcome {
            uploaded: 0,
            queue_results: Vec::new(),
            pending_album_unit: None,
        });
    };
    let (uploaded, status, failure_reason, remote_id, pending_album_unit, durable_queue_results) =
        match upload_result {
            Ok(remote_id_str) => {
                let remote_id = crate::immich::RemoteAssetId::new(remote_id_str);
                match begin_upload_unit_completion_or_fail(
                    &pool,
                    &app,
                    &audit_context,
                    unit.clone(),
                    remote_id.clone(),
                    unit.assets().count(),
                )
                .await?
                {
                    UploadUnitCompletion::PendingAlbum(pending) => {
                        (0, "PENDING", None, Some(remote_id), Some(*pending), None)
                    }
                    UploadUnitCompletion::Complete(completion) => (
                        completion.uploaded_count(),
                        "SYNCED",
                        None,
                        Some(remote_id),
                        None,
                        Some(completion.queue_results().to_vec()),
                    ),
                    UploadUnitCompletion::Failed { completion, reason } => (
                        0,
                        "FAILED",
                        Some(reason),
                        Some(remote_id),
                        None,
                        Some(completion.queue_results().to_vec()),
                    ),
                }
            }
            Err(error) => {
                log_to_ui_file_error(
                    &app,
                    "ERROR",
                    &format!("Upload failed for {}: {}", unit.image.path, error),
                    "log_upload_failed",
                    &unit.image.path,
                    &error,
                );
                for asset in unit.assets() {
                    let _ = db::mark_sync_failed(
                        &pool,
                        &asset.path,
                        asset.mtime,
                        asset.size as i64,
                        &error,
                    )
                    .await;
                }
                (0, "FAILED", Some(error), None, None, None)
            }
        };
    let mut details = upload_details(
        &unit.image,
        unit.image.checksums.as_ref().unwrap(),
        &operation_id,
        &started,
        Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)),
        remote_id.as_deref(),
        failure_reason
            .as_deref()
            .map(|error| audit_context.safe_error(error)),
    );
    if let Some(video) = &unit.live_photo_video {
        details["live_photo_local_path"] = json!(video.path);
    }
    audit_event(
        &app,
        audit_context.event(
            "file.upload.completed",
            if status != "FAILED" {
                audit::Outcome::Success
            } else {
                audit::Outcome::Failure
            },
            if status != "FAILED" {
                audit::Severity::Info
            } else {
                audit::Severity::Error
            },
            "Upload completed",
            details,
        ),
    );
    progress.advance(&app, unit.byte_size());
    let queue_results = durable_queue_results.unwrap_or_else(|| {
        if pending_album_unit.is_some() {
            return Vec::new();
        }
        unit.assets()
            .map(|asset| (asset.path.clone(), status.to_string()))
            .collect()
    });
    Ok(UploadOutcome {
        uploaded,
        queue_results,
        pending_album_unit,
    })
}

async fn begin_upload_unit_completion_or_fail(
    pool: &sqlx::SqlitePool,
    app: &tauri::AppHandle,
    audit_context: &SyncAuditContext,
    unit: HashedUploadUnit,
    remote_id: crate::immich::RemoteAssetId,
    uploaded_count: usize,
) -> Result<UploadUnitCompletion, String> {
    match completion::begin_upload_unit_completion(
        pool,
        unit.clone(),
        remote_id.clone(),
        uploaded_count,
    )
    .await
    {
        Ok(completion) => Ok(completion),
        Err(error) => {
            log_to_ui_file_error(
                app,
                "ERROR",
                &format!(
                    "Could not complete synchronization for '{}': {error}",
                    unit.image.path
                ),
                "log_upload_completion_persist_failed",
                &unit.image.path,
                &error,
            );
            audit_event(
                app,
                audit_context.event(
                    "upload.unit.completion.failed",
                    audit::Outcome::Failure,
                    audit::Severity::Error,
                    "Could not durably complete Upload Unit",
                    json!({
                        "local_path": unit.image.path,
                        "remote_asset_id": remote_id.to_string(),
                        "failure_reason": audit_context.safe_error(&error),
                    }),
                ),
            );
            completion::fail_upload_unit(pool, &unit, &error)
                .await
                .map(|completion| UploadUnitCompletion::Failed {
                    completion,
                    reason: error,
                })
        }
    }
}

async fn await_or_cancellation<T>(
    cancellation: CancellationToken,
    operation: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        result = operation => Some(result),
    }
}

pub(crate) async fn load_upload_parallelism(pool: &sqlx::SqlitePool) -> Result<usize, String> {
    db::get_upload_parallelism(pool)
        .await
        .map_err(|error| format!("Could not load upload parallelism: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload_unit(path: &str) -> UploadUnit {
        UploadUnit {
            image: crate::media::model::Asset {
                path: path.to_string(),
                size: 1,
                mtime: 1,
                target_album_id: None,
                remote_id: None,
            },
            live_photo_video: None,
        }
    }

    fn hashed_upload_unit(path: &str) -> HashedUploadUnit {
        HashedUploadUnit {
            image: crate::media::model::SyncAsset {
                path: path.to_string(),
                size: 1,
                checksums: None,
                mtime: 1,
                target_album_id: None,
                remote_id: None,
            },
            live_photo_video: None,
        }
    }

    #[test]
    fn block_summary_preserves_terminal_error_records() {
        let summary = summarize_block(
            vec![
                ("photo.jpg".to_string(), "SYNCED".to_string()),
                ("broken.jpg".to_string(), "FAILED".to_string()),
            ],
            1,
        );

        assert_eq!(summary.processed, 2);
        assert_eq!(summary.uploaded, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(
            summary.queue_results,
            vec![
                ("photo.jpg".to_string(), "SYNCED".to_string()),
                ("broken.jpg".to_string(), "FAILED".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn pipeline_upload_parallelism_loader_uses_the_configured_value() {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{}?mode=memory&cache=private",
                database_name
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert_eq!(load_upload_parallelism(&pool).await.unwrap(), 3);
        db::set_upload_parallelism(&pool, 8).await.unwrap();
        assert_eq!(load_upload_parallelism(&pool).await.unwrap(), 8);
    }

    #[test]
    fn upload_audit_details_include_only_immich_checksum() {
        let asset = crate::media::model::SyncAsset {
            path: "C:/media/photo.jpg".to_string(),
            size: 123,
            checksums: None,
            mtime: 0,
            target_album_id: None,
            remote_id: None,
        };
        let checksums = crate::media::model::Checksums {
            sha1_base64: "c2hhMQ==".to_string(),
        };
        let details = upload_details(
            &asset,
            &checksums,
            "file-operation",
            "2026-10-04T00:00:00.000000000Z",
            Some("2026-10-04T00:00:01.000000000Z".to_string()),
            Some("remote-id"),
            None,
        );
        assert_eq!(details["file_size_bytes"], 123);
        assert_eq!(details["checksums"]["sha1_base64"], checksums.sha1_base64);
        assert!(details["checksums"].get("md5").is_none());
        assert!(details["checksums"].get("sha256").is_none());
        assert_eq!(details["remote_asset_id"], "remote-id");
        assert!(details.get("failure_reason").is_none());
    }

    #[test]
    fn bulk_check_chunk_size_is_bounded_to_100() {
        assert_eq!(BULK_CHECK_CHUNK_SIZE, 100);
    }

    #[test]
    fn pause_before_hashing_prevents_new_hashes() {
        let coordinator = SyncCoordinatorInner::new(true);
        assert!(!can_start_work(&coordinator));
    }

    #[tokio::test]
    async fn paused_processor_observes_a_resume_notified_before_its_pause_check() {
        let coordinator = Arc::new(SyncCoordinatorInner::new(true));
        let waiting_coordinator = coordinator.clone();
        let waiting = tokio::spawn(async move { wait_while_paused(&waiting_coordinator).await });
        tokio::task::yield_now().await;

        coordinator.paused.store(false, Ordering::SeqCst);
        coordinator.resume.notify_waiters();

        tokio::time::timeout(std::time::Duration::from_millis(50), waiting)
            .await
            .expect("the processor must not miss a concurrent resume")
            .unwrap();
    }

    #[test]
    fn pause_during_hashing_prevents_follow_up_hashes() {
        let coordinator = SyncCoordinatorInner::new(false);
        assert!(can_start_work(&coordinator));
        coordinator.paused.store(true, Ordering::SeqCst);
        assert!(!can_start_work(&coordinator));
    }

    #[test]
    fn pause_before_bulk_check_prevents_new_request() {
        let coordinator = SyncCoordinatorInner::new(true);
        assert!(!can_start_work(&coordinator));
    }

    #[test]
    fn pause_during_uploads_prevents_follow_up_uploads() {
        let coordinator = SyncCoordinatorInner::new(false);
        assert!(can_start_work(&coordinator));
        coordinator.paused.store(true, Ordering::SeqCst);
        assert!(!can_start_work(&coordinator));
    }

    #[tokio::test]
    async fn cancellation_aborts_a_blocking_upload_without_a_terminal_queue_result() {
        let cancellation = CancellationToken::new();
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let entered_by_upload = entered.clone();
        let blocking_upload = async move {
            entered_by_upload.notify_one();
            std::future::pending::<Result<String, String>>().await
        };
        let cancellation_for_upload = cancellation.clone();
        let upload = tokio::spawn(async move {
            await_or_cancellation(cancellation_for_upload, blocking_upload).await
        });

        entered.notified().await;
        cancellation.cancel();

        // None is intentionally mapped by upload_unit to an empty queue result, so it
        // cannot create a failed/retryable persistence record.
        assert!(upload.await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cancellation_before_hashing_leaves_no_terminal_queue_result() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let outcome = await_or_cancellation(cancellation, async {
            HashOutcome {
                unit: None,
                queue_results: vec![("photo.jpg".into(), "FAILED".into())],
            }
        })
        .await;

        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn cancellation_during_hashing_leaves_no_terminal_queue_result() {
        let cancellation = CancellationToken::new();
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let entered_by_hash = entered.clone();
        let hashing = tokio::spawn(await_or_cancellation(cancellation.clone(), async move {
            entered_by_hash.notify_one();
            std::future::pending::<HashOutcome>().await
        }));

        entered.notified().await;
        cancellation.cancel();

        assert!(hashing.await.unwrap().is_none());
    }

    #[tokio::test]
    async fn bulk_check_keeps_other_folder_work_when_one_folder_is_cancelled() {
        let work = Arc::new(crate::sync::ActiveWork::new());
        let cancelled_folder = upload_unit("C:/cancelled/photo.jpg");
        let active_folder = upload_unit("C:/kept/photo.jpg");
        let lease = work.acquire(&[cancelled_folder.clone(), active_folder.clone()]);
        let active_work = lease.context();
        let mut wait_for_all = Box::pin(tokio::spawn(wait_for_all_cancellations(vec![
            active_work
                .for_upload_unit(&cancelled_folder)
                .cancellation(),
            active_work.for_upload_unit(&active_folder).cancellation(),
        ])));

        work.cancel_work_in("C:/cancelled");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut wait_for_all)
                .await
                .is_err()
        );

        work.cancel_work_in("C:/kept");
        wait_for_all.await.unwrap();
    }

    #[test]
    fn upload_refill_skips_cancelled_folder_items_until_the_pool_is_full() {
        let work = Arc::new(crate::sync::ActiveWork::new());
        let cancelled_first = upload_unit("C:/cancelled/first.jpg");
        let cancelled_second = upload_unit("C:/cancelled/second.jpg");
        let kept_first = upload_unit("C:/kept/first.jpg");
        let kept_second = upload_unit("C:/kept/second.jpg");
        let lease = work.acquire(&[cancelled_first, cancelled_second, kept_first, kept_second]);
        work.cancel_work_in("C:/cancelled");
        let mut pending = vec![
            hashed_upload_unit("C:/cancelled/first.jpg"),
            hashed_upload_unit("C:/cancelled/second.jpg"),
            hashed_upload_unit("C:/kept/first.jpg"),
            hashed_upload_unit("C:/kept/second.jpg"),
        ]
        .into_iter();
        let active_work = lease.context();
        let mut scheduled_paths = Vec::new();
        const UPLOAD_PARALLELISM: usize = 2;

        while scheduled_paths.len() < UPLOAD_PARALLELISM {
            let Some((unit, _)) = next_active_upload_unit(&mut pending, &active_work) else {
                break;
            };
            scheduled_paths.push(unit.image.path);
        }

        assert_eq!(scheduled_paths, ["C:/kept/first.jpg", "C:/kept/second.jpg"]);
    }

    #[tokio::test]
    async fn cancelling_one_upload_does_not_stop_another_folder() {
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let other_folder = CancellationToken::new();

        assert!(
            await_or_cancellation(cancelled, std::future::pending::<()>())
                .await
                .is_none()
        );
        assert_eq!(
            await_or_cancellation(other_folder, async { "uploaded" }).await,
            Some("uploaded")
        );
    }

    #[tokio::test]
    async fn cancelled_folder_paths_are_skipped_before_follow_up_uploads() {
        let work = std::sync::Arc::new(crate::sync::ActiveWork::new());
        let removed = crate::media::model::UploadUnit {
            image: crate::media::model::Asset {
                path: "C:/removed/second.jpg".into(),
                size: 1,
                mtime: 1,
                target_album_id: None,
                remote_id: None,
            },
            live_photo_video: None,
        };
        let kept = crate::media::model::UploadUnit {
            image: crate::media::model::Asset {
                path: "C:/kept/third.jpg".into(),
                size: 1,
                mtime: 1,
                target_album_id: None,
                remote_id: None,
            },
            live_photo_video: None,
        };
        let lease = work.acquire(&[removed.clone(), kept.clone()]);
        work.cancel_work_in("C:/removed");

        assert!(lease.context().for_upload_unit(&removed).is_cancelled());
        assert!(!lease.context().for_upload_unit(&kept).is_cancelled());
    }

    #[test]
    fn pending_album_unit_collects_only_primary_id_for_live_photo() {
        let image = crate::media::model::SyncAsset {
            path: "C:/photos/live.jpg".to_string(),
            size: 500,
            checksums: Some(crate::media::model::Checksums {
                sha1_base64: "img-hash".to_string(),
            }),
            mtime: 100,
            target_album_id: Some(crate::immich::AlbumId::new("album-123")),
            remote_id: Some(crate::immich::RemoteAssetId::new("remote-img-uuid")),
        };
        let video = crate::media::model::SyncAsset {
            path: "C:/photos/live.mov".to_string(),
            size: 1500,
            checksums: Some(crate::media::model::Checksums {
                sha1_base64: "vid-hash".to_string(),
            }),
            mtime: 100,
            target_album_id: Some(crate::immich::AlbumId::new("album-123")),
            remote_id: None,
        };
        let unit = HashedUploadUnit {
            image: image.clone(),
            live_photo_video: Some(video.clone()),
        };

        let pending = PendingAlbumUnit {
            album_id: crate::immich::AlbumId::new("album-123"),
            remote_id: crate::immich::RemoteAssetId::new("remote-img-uuid"),
            unit: unit.clone(),
            uploaded_count: 2,
        };

        assert_eq!(pending.album_id.as_str(), "album-123");
        assert_eq!(pending.remote_id.as_str(), "remote-img-uuid");
        assert_eq!(pending.uploaded_count, 2);
        let paths: Vec<String> = pending.unit.assets().map(|a| a.path.clone()).collect();
        assert_eq!(paths, vec!["C:/photos/live.jpg", "C:/photos/live.mov"]);
    }
}
