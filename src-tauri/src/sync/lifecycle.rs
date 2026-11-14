//! Ownership and durable transitions for the synchronization runner.

use crate::db;
use std::sync::{atomic::Ordering, Arc};

#[derive(Debug, Clone)]
pub(crate) enum LifecycleClaim {
    Started(db::SyncSnapshot),
    Joined,
    Paused,
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
        db::recover_sync_job(pool).await
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

    pub(crate) async fn start_or_join(&self) -> Result<LifecycleClaim, String> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let current = self.status().await?;
        if current.status == "PAUSED" {
            return Ok(LifecycleClaim::Paused);
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(LifecycleClaim::Joined);
        }

        match db::set_sync_status(self.pool, "RUNNING").await {
            Ok(snapshot) => Ok(LifecycleClaim::Started(snapshot)),
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                Err(error.to_string())
            }
        }
    }

    pub(crate) async fn complete(&self) -> Result<Option<db::SyncSnapshot>, String> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = db::complete_sync_job_if_finished(self.pool)
            .await
            .map_err(|error| error.to_string())?;
        if snapshot.is_some() {
            self.running.store(false, Ordering::SeqCst);
        }
        Ok(snapshot)
    }

    pub(crate) async fn fail(&self) -> Result<db::SyncSnapshot, String> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let result = db::set_sync_status(self.pool, "FAILED")
            .await
            .map_err(|error| error.to_string());
        self.running.store(false, Ordering::SeqCst);
        self.coordinator.paused.store(false, Ordering::SeqCst);
        self.coordinator.resume.notify_waiters();
        result
    }

    pub(crate) async fn pause(&self) -> Result<db::SyncSnapshot, String> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = db::set_sync_status(self.pool, "PAUSED")
            .await
            .map_err(|error| error.to_string())?;
        self.coordinator.paused.store(true, Ordering::SeqCst);
        Ok(snapshot)
    }

    pub(crate) async fn resume(&self) -> Result<db::SyncSnapshot, String> {
        let _guard = self.coordinator.lifecycle_lock.lock().await;
        let snapshot = self.status().await?;
        if snapshot.status != "PAUSED" {
            return Ok(snapshot);
        }

        let snapshot = db::set_sync_status(self.pool, "RUNNING")
            .await
            .map_err(|error| error.to_string())?;
        self.coordinator.paused.store(false, Ordering::SeqCst);
        self.coordinator.resume.notify_waiters();
        Ok(snapshot)
    }

    pub(crate) async fn status(&self) -> Result<db::SyncSnapshot, String> {
        db::get_sync_snapshot(self.pool)
            .await
            .map_err(|error| error.to_string())
    }
}
