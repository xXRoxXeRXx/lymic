//! Ownership and durable transitions for the synchronization runner.

use crate::{
    db::{self, SyncJobStatus},
    sync::SyncRuntime,
};

#[derive(Debug)]
pub(crate) enum LifecycleError {
    Database(sqlx::Error),
    Admission(String),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => error.fmt(formatter),
            Self::Admission(error) => formatter.write_str(error),
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
    runtime: &'a SyncRuntime,
}

impl SyncRuntime {
    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        SynchronizationLifecycle::new(self).status().await
    }

    pub(crate) async fn discard_folder(&self, path: &str) -> Result<(), LifecycleError> {
        SynchronizationLifecycle::new(self)
            .discard_folder(path)
            .await
    }
}

impl<'a> SynchronizationLifecycle<'a> {
    pub(crate) async fn recover(pool: &sqlx::SqlitePool) -> Result<db::SyncSnapshot, sqlx::Error> {
        // In-flight transfers and admitted-but-unclaimed work require confirmation after restart.
        sqlx::query("UPDATE sync_jobs SET status = 'PAUSED' WHERE id = 1 AND (status = 'RUNNING' OR (status IN ('IDLE', 'FAILED') AND EXISTS(SELECT 1 FROM sync_queue WHERE job_id = 1 AND status = 'PENDING')))")
            .execute(pool).await?;
        db::get_sync_snapshot(pool).await
    }

    pub(crate) fn new(runtime: &'a SyncRuntime) -> Self {
        Self { runtime }
    }

    pub(crate) async fn start_or_join(&self) -> Result<LifecycleClaim, LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        self.start_or_join_locked().await
    }

    async fn start_or_join_locked(&self) -> Result<LifecycleClaim, LifecycleError> {
        let current = self.status().await?;
        if current.status == SyncJobStatus::Paused {
            return Ok(LifecycleClaim::Paused);
        }
        if !self.runtime.try_claim_run() {
            return Ok(LifecycleClaim::Joined);
        }

        match db::set_sync_status(self.runtime.pool(), SyncJobStatus::Running).await {
            Ok(snapshot) => Ok(LifecycleClaim::Started(snapshot)),
            Err(error) => {
                self.runtime.clear_running();
                Err(error.into())
            }
        }
    }

    pub(crate) async fn admit_and_claim(
        &self,
        scan: &crate::media::model::ScanResult,
    ) -> Result<(Option<db::SyncSnapshot>, LifecycleClaim), LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        let pool = self.runtime.pool();
        let admitted = if scan.files.is_empty() && scan.failures.is_empty() {
            if !db::has_pending_sync_queue_items(pool).await? {
                return Ok((None, LifecycleClaim::NoWork));
            }
            None
        } else {
            Some(
                crate::sync::service::admit_scan_result(pool, scan)
                    .await
                    .map_err(LifecycleError::Admission)?,
            )
        };
        Ok((admitted, self.start_or_join_locked().await?))
    }

    pub(crate) async fn discard_failed(&self, paths: &[String]) -> Result<(), LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        db::discard_failed_sync_paths(self.runtime.pool(), paths).await?;
        self.reconcile_empty_job().await
    }

    pub(crate) async fn discard_folder(&self, path: &str) -> Result<(), LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        db::discard_sync_data_in_folder(self.runtime.pool(), path).await?;
        self.reconcile_empty_job().await
    }

    async fn reconcile_empty_job(&self) -> Result<(), LifecycleError> {
        let pool = self.runtime.pool();
        if !self.runtime.is_running()
            && self.status().await?.status != SyncJobStatus::Paused
            && !db::has_pending_sync_queue_items(pool).await?
        {
            db::set_sync_status(pool, SyncJobStatus::Idle).await?;
        }
        Ok(())
    }

    pub(crate) async fn complete(&self) -> Result<Option<db::SyncSnapshot>, LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        let snapshot = db::complete_sync_job_if_finished(self.runtime.pool()).await?;
        if snapshot.is_some() {
            self.runtime.clear_running();
        }
        Ok(snapshot)
    }

    pub(crate) async fn fail(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        let result = db::set_sync_status(self.runtime.pool(), SyncJobStatus::Failed)
            .await
            .map_err(LifecycleError::from);
        self.runtime.clear_running();
        self.runtime.resume_work();
        result
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        let snapshot = db::set_sync_status(self.runtime.pool(), SyncJobStatus::Paused).await?;
        self.runtime.pause_work();
        Ok(snapshot)
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        let _guard = self.runtime.lock_lifecycle().await;
        let snapshot = self.status().await?;
        if snapshot.status != SyncJobStatus::Paused {
            return Ok(snapshot);
        }

        let snapshot = db::set_sync_status(self.runtime.pool(), SyncJobStatus::Running).await?;
        self.runtime.resume_work();
        Ok(snapshot)
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, LifecycleError> {
        db::get_sync_snapshot(self.runtime.pool())
            .await
            .map_err(LifecycleError::from)
    }
}
