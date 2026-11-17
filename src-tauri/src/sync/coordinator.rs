use crate::{
    db,
    media::model::UploadUnit,
    sync::{
        active_work::{ActiveWork, ActiveWorkLease},
        reporter::SyncRunReporter,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Shared handle owning Synchronization's database pool, run claim, pause
/// signaling, and active-work registry behind behavioral operations.
#[derive(Clone)]
pub(crate) struct SyncRuntime {
    inner: Arc<SyncRuntimeInner>,
}

struct SyncRuntimeInner {
    pool: sqlx::SqlitePool,
    running: AtomicBool,
    lock: tokio::sync::Mutex<()>,
    lifecycle_lock: tokio::sync::Mutex<()>,
    paused: AtomicBool,
    resume: tokio::sync::Notify,
    active_work: Arc<ActiveWork>,
}

impl SyncRuntime {
    pub(crate) fn new(pool: sqlx::SqlitePool, paused: bool) -> Self {
        Self {
            inner: Arc::new(SyncRuntimeInner {
                pool,
                running: AtomicBool::new(false),
                lock: tokio::sync::Mutex::new(()),
                lifecycle_lock: tokio::sync::Mutex::new(()),
                paused: AtomicBool::new(paused),
                resume: tokio::sync::Notify::new(),
                active_work: Arc::new(ActiveWork::new()),
            }),
        }
    }

    pub(crate) async fn recover(
        pool: sqlx::SqlitePool,
    ) -> Result<(Self, db::SyncSnapshot), sqlx::Error> {
        let snapshot = crate::sync::lifecycle::SynchronizationLifecycle::recover(&pool).await?;
        let runtime = Self::new(pool, snapshot.status == db::SyncJobStatus::Paused);
        Ok((runtime, snapshot))
    }

    pub(crate) fn pool(&self) -> &sqlx::SqlitePool {
        &self.inner.pool
    }

    pub(crate) fn can_start_work(&self) -> bool {
        !self.inner.paused.load(Ordering::SeqCst)
    }

    pub(crate) async fn wait_while_paused(&self) {
        while self.inner.paused.load(Ordering::SeqCst) {
            let notified = self.inner.resume.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.inner.paused.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
    }

    pub(in crate::sync) async fn wait_while_paused_reporting(
        &self,
        reporter: &impl SyncRunReporter,
    ) -> Result<(), String> {
        while self.inner.paused.load(Ordering::SeqCst) {
            let notified = self.inner.resume.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.inner.paused.load(Ordering::SeqCst) {
                break;
            }
            let snapshot = db::get_sync_snapshot(&self.inner.pool)
                .await
                .map_err(|error| format!("Could not load paused sync: {error}"))?;
            if !self.inner.paused.load(Ordering::SeqCst) {
                continue;
            }
            reporter.paused(&snapshot);
            if self.inner.paused.load(Ordering::SeqCst) {
                notified.await;
            }
            if !self.inner.paused.load(Ordering::SeqCst) {
                let snapshot = db::get_sync_snapshot(&self.inner.pool)
                    .await
                    .map_err(|error| format!("Could not load resumed sync: {error}"))?;
                reporter.resumed(&snapshot);
            }
        }
        Ok(())
    }

    pub(in crate::sync) async fn lock_run(
        &self,
        on_waiting: impl FnOnce(),
    ) -> tokio::sync::MutexGuard<'_, ()> {
        match self.inner.lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                on_waiting();
                self.inner.lock.lock().await
            }
        }
    }

    pub(crate) fn has_active_work_in(&self, folder: &str) -> bool {
        self.inner.active_work.has_work_in(folder)
    }

    pub(crate) async fn has_work_in_folder(&self, folder: &str) -> Result<bool, String> {
        if self.has_active_work_in(folder) {
            return Ok(true);
        }
        db::has_sync_work_in_folder(&self.inner.pool, folder)
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) fn cancel_work_in(&self, folder: &str) {
        self.inner.active_work.cancel_work_in(folder);
    }

    pub(crate) fn acquire_active_work(&self, units: &[UploadUnit]) -> ActiveWorkLease {
        self.inner.active_work.acquire(units)
    }

    pub(in crate::sync) async fn lock_lifecycle(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.lifecycle_lock.lock().await
    }

    pub(in crate::sync) fn try_claim_run(&self) -> bool {
        self.inner
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(in crate::sync) fn clear_running(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
    }

    pub(in crate::sync) fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    pub(in crate::sync) fn pause_work(&self) {
        self.inner.paused.store(true, Ordering::SeqCst);
    }

    pub(in crate::sync) fn resume_work(&self) {
        self.inner.paused.store(false, Ordering::SeqCst);
        self.inner.resume.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::SyncRuntime;
    use crate::{
        db,
        media::model::{Asset, UploadUnit},
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    async fn test_pool() -> sqlx::SqlitePool {
        let database_name = format!("lymic_runtime_test_{}", uuid::Uuid::new_v4());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:file:{database_name}?mode=memory&cache=private"
            ))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn unit(path: &str) -> UploadUnit {
        UploadUnit {
            image: Asset {
                path: path.to_string(),
                size: 1,
                mtime: 1,
                target_album_id: None,
                remote_id: None,
            },
            live_photo_video: None,
        }
    }

    #[tokio::test]
    async fn runtime_handle_gates_work_until_resumed() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool, true);
        assert!(!runtime.can_start_work());

        let waiter_runtime = runtime.clone();
        let waiting = tokio::spawn(async move { waiter_runtime.wait_while_paused().await });
        tokio::task::yield_now().await;

        runtime.resume_work();
        assert!(runtime.can_start_work());
        tokio::time::timeout(std::time::Duration::from_millis(100), waiting)
            .await
            .expect("wait_while_paused must return once resumed")
            .unwrap();
    }

    #[tokio::test]
    async fn runtime_handle_tracks_and_cancels_active_work_by_folder() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool, false);
        let photos_unit = unit("C:/Photos/active.jpg");
        let videos_unit = unit("C:/Videos/active.jpg");
        let lease = runtime.acquire_active_work(&[photos_unit.clone(), videos_unit.clone()]);

        assert!(runtime.has_active_work_in("C:/Photos"));
        assert!(runtime.has_work_in_folder("C:/Photos").await.unwrap());

        runtime.cancel_work_in("C:/Photos");

        assert!(lease.context().for_upload_unit(&photos_unit).is_cancelled());
        assert!(!lease.context().for_upload_unit(&videos_unit).is_cancelled());

        drop(lease);
        assert!(!runtime.has_active_work_in("C:/Photos"));
        assert!(!runtime.has_work_in_folder("C:/Photos").await.unwrap());
    }

    #[tokio::test]
    async fn runtime_handle_recovery_initializes_paused_state_from_persisted_work() {
        let pool = test_pool().await;
        db::enqueue_sync_assets(
            &pool,
            &[db::QueueAsset {
                path: "C:/Photos/pending.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            }],
            &[],
        )
        .await
        .unwrap();

        let (runtime, snapshot) = SyncRuntime::recover(pool).await.unwrap();
        assert_eq!(snapshot.status, db::SyncJobStatus::Paused);
        assert!(!runtime.can_start_work());
        assert!(!runtime.is_running());
    }

    #[tokio::test]
    async fn runtime_handle_run_lock_notifies_only_when_contended() {
        let pool = test_pool().await;
        let runtime = SyncRuntime::new(pool, false);

        let waited_first = Arc::new(AtomicBool::new(false));
        let guard = runtime
            .lock_run(|| waited_first.store(true, Ordering::SeqCst))
            .await;
        assert!(!waited_first.load(Ordering::SeqCst));

        let waited_second = Arc::new(AtomicBool::new(false));
        let waited_second_flag = waited_second.clone();
        let contender_runtime = runtime.clone();
        let contender = tokio::spawn(async move {
            let _guard = contender_runtime
                .lock_run(|| waited_second_flag.store(true, Ordering::SeqCst))
                .await;
        });

        tokio::task::yield_now().await;
        assert!(waited_second.load(Ordering::SeqCst));
        drop(guard);
        contender.await.unwrap();
    }
}
