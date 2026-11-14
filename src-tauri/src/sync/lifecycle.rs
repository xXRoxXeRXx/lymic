//! Ownership and durable transitions for the synchronization runner.

use crate::db::{self, SyncJobStatus};
use std::sync::{atomic::Ordering, Arc};

#[derive(Debug)]
pub(crate) enum LifecycleError {
    Database(sqlx::Error),
    Admission(String),
    InvalidStatus(String),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => error.fmt(formatter),
            Self::Admission(error) | Self::InvalidStatus(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for LifecycleError {}

impl From<sqlx::Error> for LifecycleError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<LifecycleError> for String {
    fn from(error: LifecycleError) -> Self {
        error.to_string()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum LifecycleClaim {
    Started(db::SyncSnapshot),
    Joined,
    Paused,
    NoWork,
}

/// Durable synchronization state transitions, independent of the desktop shell.
///
/// `Synchronization` owns the surrounding desktop orchestration; this smaller
/// collaborator is deliberately kept free of Tauri so its state-machine
/// contract can be exercised against the real SQLite schema.
pub(crate) struct SynchronizationLifecycle<'a> {
    pool: &'a sqlx::SqlitePool,
    running: &'a std::sync::atomic::AtomicBool,
    coordinator: Arc<crate::sync::SyncCoordinatorInner>,
}

impl<'a> SynchronizationLifecycle<'a> {
    pub(crate) async fn recover(pool: &sqlx::SqlitePool) -> Result<db::SyncSnapshot, sqlx::Error> {
        // In-flight transfers and admitted-but-unclaimed work require confirmation after restart.
        sqlx::query("UPDATE sync_jobs SET status = 'PAUSED' WHERE id = 1 AND (status = 'RUNNING' OR (status IN ('IDLE', 'FAILED') AND EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING')))")
            .execute(pool).await?;
        db::get_sync_snapshot(pool).await
    }

    pub(crate) fn new(
        pool: &'a sqlx::SqlitePool,
        running: &'a std::sync::atomic::AtomicBool,
        coordinator: Arc<crate::sync::SyncCoordinatorInner>,
    ) -> Self {
        Self {
            pool,
            running,
            coordinator,
        }
    }

    pub(crate) async fn start_or_join(&self) -> Result<LifecycleClaim, LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        self.start_or_join_locked().await
    }

    async fn start_or_join_locked(&self) -> Result<LifecycleClaim, LifecycleError> {
        let current = self.status().await?;
        if current
            .status
            .parse::<SyncJobStatus>()
            .map_err(LifecycleError::InvalidStatus)?
            == SyncJobStatus::Paused
        {
            return Ok(LifecycleClaim::Paused);
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(LifecycleClaim::Joined);
        }

        match db::set_sync_status(self.pool, SyncJobStatus::Running).await {
            Ok(snapshot) => Ok(LifecycleClaim::Started(snapshot)),
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                Err(error.into())
            }
        }
    }

    pub(crate) async fn admit_and_claim(
        &self,
        scan: &crate::sync::ScanResult,
    ) -> Result<(Option<db::SyncSnapshot>, LifecycleClaim), LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let admitted = if scan.files.is_empty() && scan.failures.is_empty() {
            if !db::has_pending_sync_queue_items(self.pool).await? {
                return Ok((None, LifecycleClaim::NoWork));
            }
            None
        } else {
            Some(
                crate::sync::service::admit_scan_result(self.pool, scan)
                    .await
                    .map_err(LifecycleError::Admission)?,
            )
        };
        Ok((admitted, self.start_or_join_locked().await?))
    }

    pub(crate) async fn discard_failed(&self, paths: &[String]) -> Result<(), LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        db::discard_failed_sync_paths(self.pool, paths).await?;
        self.reconcile_empty_job().await
    }

    pub(crate) async fn discard_folder(&self, path: &str) -> Result<(), LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        db::discard_sync_data_in_folder(self.pool, path).await?;
        self.reconcile_empty_job().await
    }

    async fn reconcile_empty_job(&self) -> Result<(), LifecycleError> {
        if !self.running.load(Ordering::SeqCst)
            && self
                .status()
                .await?
                .status
                .parse::<SyncJobStatus>()
                .map_err(LifecycleError::InvalidStatus)?
                != SyncJobStatus::Paused
            && !db::has_pending_sync_queue_items(self.pool).await?
        {
            db::set_sync_status(self.pool, SyncJobStatus::Idle).await?;
        }
        Ok(())
    }

    pub(crate) async fn complete(&self) -> Result<Option<db::SyncSnapshot>, LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = db::complete_sync_job_if_finished(self.pool).await?;
        if snapshot.is_some() {
            self.running.store(false, Ordering::SeqCst);
        }
        Ok(snapshot)
    }

    pub(crate) async fn fail(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let result = db::set_sync_status(self.pool, SyncJobStatus::Failed)
            .await
            .map_err(LifecycleError::from);
        self.running.store(false, Ordering::SeqCst);
        self.coordinator.paused.store(false, Ordering::SeqCst);
        self.coordinator.resume.notify_waiters();
        result
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = db::set_sync_status(self.pool, SyncJobStatus::Paused).await?;
        self.coordinator.paused.store(true, Ordering::SeqCst);
        Ok(snapshot)
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = self.status().await?;
        if snapshot
            .status
            .parse::<SyncJobStatus>()
            .map_err(LifecycleError::InvalidStatus)?
            != SyncJobStatus::Paused
        {
            return Ok(snapshot);
        }

        let snapshot = db::set_sync_status(self.pool, SyncJobStatus::Running).await?;
        self.coordinator.paused.store(false, Ordering::SeqCst);
        self.coordinator.resume.notify_waiters();
        Ok(snapshot)
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        db::get_sync_snapshot(self.pool)
            .await
            .map_err(LifecycleError::from)
    }
}
