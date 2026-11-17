//! Durable synchronization queue execution.

use crate::{
    audit::SyncAuditContext,
    db,
    immich::{ImmichClient, SidecarDirectoryCache},
    media::model::UploadUnit,
    sync::{
        processor::{RunProcessor, SyncRunContext, SyncSummary},
        reporter::{DesktopRunProgress, DesktopRunReporter, SyncRunReporter},
        CurrentFileProgress, JobByteProgress, ProcessingContext, SyncRuntime, SyncSource,
    },
};
use std::sync::Arc;

const SYNC_QUEUE_BLOCK_SIZE: i64 = 50;

pub(crate) trait QueueBlockProcessor {
    fn prepare(&self) -> impl std::future::Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    fn process_block(
        &self,
        units: Vec<UploadUnit>,
        active_work: ProcessingContext,
    ) -> impl std::future::Future<Output = Result<SyncSummary, String>> + Send;
}

impl<R: tauri::Runtime> QueueBlockProcessor for RunProcessor<R> {
    fn prepare(&self) -> impl std::future::Future<Output = Result<(), String>> + Send {
        self.prepare()
    }

    fn process_block(
        &self,
        units: Vec<UploadUnit>,
        active_work: ProcessingContext,
    ) -> impl std::future::Future<Output = Result<SyncSummary, String>> + Send {
        self.process(units, active_work)
    }
}

pub(crate) async fn run_queue_loop<P, R>(
    runtime: &SyncRuntime,
    source: SyncSource,
    processor: &P,
    reporter: &R,
) -> Result<SyncSummary, String>
where
    P: QueueBlockProcessor,
    R: SyncRunReporter,
{
    match run_queue_loop_inner(runtime, source, processor, reporter).await {
        Ok(summary) => Ok(summary),
        Err(error) => {
            fail_and_report(runtime, reporter, &error).await;
            Err(error)
        }
    }
}

async fn fail_and_report<R: SyncRunReporter>(runtime: &SyncRuntime, reporter: &R, error: &str) {
    let snapshot = runtime.fail().await.ok();
    reporter.failed(snapshot.as_ref(), error);
}

async fn run_queue_loop_inner<P, R>(
    runtime: &SyncRuntime,
    source: SyncSource,
    processor: &P,
    reporter: &R,
) -> Result<SyncSummary, String>
where
    P: QueueBlockProcessor,
    R: SyncRunReporter,
{
    let pool = runtime.pool();
    let snapshot = db::get_sync_snapshot(pool)
        .await
        .map_err(|error| format!("Could not load sync queue: {error}"))?;
    let _sync_guard = runtime
        .lock_run(|| {
            if source.is_manual() {
                reporter.queued_waiting();
            }
        })
        .await;
    reporter.started(&snapshot);
    processor.prepare().await?;
    reporter.transfers_started();
    let mut totals = SyncSummary {
        failed: snapshot.failed.max(0) as usize,
        ..Default::default()
    };
    loop {
        runtime.wait_while_paused_reporting(reporter).await?;
        let queued = db::next_queued_upload_units(pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .map_err(|error| format!("Could not load sync queue: {error}"))?;
        if queued.is_empty() {
            let Some(snapshot) = runtime.complete().await? else {
                continue;
            };
            reporter.completed(&snapshot, &totals);
            return Ok(totals);
        }
        let folders = db::get_folders(pool)
            .await
            .map_err(|error| format!("Could not load watched folders: {error}"))?;
        let excluded_paths = excluded_paths_at_queue_boundary(&queued, &folders);
        if !excluded_paths.is_empty() {
            let snapshot = db::discard_pending_sync_paths(pool, &excluded_paths)
                .await
                .map_err(|error| format!("Could not discard excluded queue entries: {error}"))?;
            reporter.progressed(&snapshot);
            continue;
        }
        let block = queued
            .into_iter()
            .map(db::QueuedUploadUnit::into_upload_unit)
            .collect::<Vec<_>>();
        let lease = runtime.acquire_active_work(&block);
        let result = processor.process_block(block, lease.context()).await;
        drop(lease);
        let result = result?;
        totals.processed += result.processed;
        totals.uploaded += result.uploaded;
        totals.failed += result.failed;
        if let Some(snapshot) = persist_processor_results(pool, &result).await? {
            reporter.progressed(&snapshot);
        }
    }
}

fn excluded_paths_at_queue_boundary(
    queued: &[db::QueuedUploadUnit],
    folders: &[db::WatchedFolder],
) -> Vec<String> {
    queued
        .iter()
        .filter(|unit| {
            crate::media::admission::decide_paths(
                std::iter::once(unit.image.local_path.as_str()).chain(
                    unit.live_photo_video
                        .as_ref()
                        .map(|asset| asset.local_path.as_str()),
                ),
                folders,
            ) == crate::media::admission::Admission::Excluded
        })
        .flat_map(|unit| {
            std::iter::once(&unit.image.local_path)
                .chain(unit.live_photo_video.iter().map(|asset| &asset.local_path))
        })
        .cloned()
        .collect()
}

/// Shared launch parameters for a Synchronization pipeline run.
pub(crate) struct SyncPipelineParams<R: tauri::Runtime = tauri::Wry> {
    pub(crate) app: tauri::AppHandle<R>,
    pub(crate) runtime: SyncRuntime,
    pub(crate) client: Arc<ImmichClient>,
    pub(crate) source: SyncSource,
    pub(crate) audit_context: SyncAuditContext,
}

pub(crate) async fn run_sync_pipeline<R: tauri::Runtime>(
    params: SyncPipelineParams<R>,
) -> Result<SyncSummary, String> {
    let SyncPipelineParams {
        app,
        runtime,
        client,
        source,
        audit_context,
    } = params;
    // Admission persists producer work before this runner starts. The durable
    // queue is therefore the sole source used here, including after recovery.
    let progress = DesktopRunProgress::new(
        app.clone(),
        JobByteProgress::new(0, 0),
        CurrentFileProgress::default(),
    );
    let reporter =
        DesktopRunReporter::new(app.clone(), source, audit_context.clone(), progress.clone());
    let processor = RunProcessor::with_context(SyncRunContext {
        app,
        runtime: runtime.clone(),
        client,
        audit_context,
        progress,
        sidecar_cache: SidecarDirectoryCache::new(),
    });
    run_queue_loop(&runtime, source, &processor, &reporter).await
}

fn terminal_queue_results(summary: &SyncSummary) -> Option<&[(String, String)]> {
    (!summary.queue_results.is_empty()).then_some(&summary.queue_results)
}

/// Commits only the terminal outcomes returned by a processor block.
///
/// A processor may stop at a pause boundary after handling part of a loaded
/// block. Leaving the remainder pending is what makes a later resume lossless.
async fn persist_processor_results(
    pool: &sqlx::SqlitePool,
    summary: &SyncSummary,
) -> Result<Option<db::SyncSnapshot>, String> {
    let Some(queue_results) = terminal_queue_results(summary) else {
        return Ok(None);
    };
    db::finalize_queued_block(pool, queue_results)
        .await
        .map(Some)
        .map_err(|error| format!("Could not persist queue block: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn queue_window_is_limited_to_fifty_items() {
        assert_eq!(SYNC_QUEUE_BLOCK_SIZE, 50);
    }

    #[test]
    fn partial_window_only_finalizes_reported_terminal_results() {
        let summary = SyncSummary {
            processed: 2,
            uploaded: 1,
            failed: 1,
            queue_results: vec![
                ("one.jpg".to_string(), "SYNCED".to_string()),
                ("two.jpg".to_string(), "FAILED".to_string()),
            ],
        };

        assert_eq!(
            terminal_queue_results(&summary),
            Some(
                [
                    ("one.jpg".to_string(), "SYNCED".to_string()),
                    ("two.jpg".to_string(), "FAILED".to_string()),
                ]
                .as_slice()
            )
        );
    }

    #[tokio::test]
    async fn processor_outcomes_finalize_only_completed_work_and_leave_pause_remainder_pending() {
        let pool = test_pool().await;
        let units = ["first.jpg", "second.jpg", "third.jpg"]
            .into_iter()
            .map(|path| db::QueueUploadUnit {
                image: db::QueueUploadAsset {
                    path: path.to_string(),
                    size: 1,
                    mtime: 1,
                    target_album_id: None,
                },
                live_photo_video: None,
            })
            .collect::<Vec<_>>();
        db::enqueue_upload_units(&pool, &units, &[]).await.unwrap();

        let snapshot = persist_processor_results(
            &pool,
            &SyncSummary {
                processed: 2,
                uploaded: 1,
                failed: 1,
                queue_results: vec![
                    ("first.jpg".to_string(), "SYNCED".to_string()),
                    ("second.jpg".to_string(), "FAILED".to_string()),
                ],
            },
        )
        .await
        .unwrap()
        .expect("terminal processor outcomes update the durable queue");

        assert_eq!((snapshot.succeeded, snapshot.failed), (1, 1));
        let remaining = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].image.local_path, "third.jpg");
    }

    #[tokio::test]
    async fn queue_boundary_discards_pending_upload_units_matching_current_exclusions() {
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("lymic-queue-test-{}", uuid::Uuid::new_v4()));
        let excluded = root.join("Screenshots/private.jpg");
        let included = root.join("Trips/keep.jpg");
        let folders = vec![db::WatchedFolder {
            id: 1,
            path: root.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec!["screenshots/**".to_string()],
        }];
        let units = [excluded, included]
            .into_iter()
            .map(|path| db::QueueUploadUnit {
                image: db::QueueUploadAsset {
                    path: path.to_string_lossy().to_string(),
                    size: 1,
                    mtime: 1,
                    target_album_id: None,
                },
                live_photo_video: None,
            })
            .collect::<Vec<_>>();
        db::enqueue_upload_units(&pool, &units, &[]).await.unwrap();

        let queued = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        let excluded_paths = excluded_paths_at_queue_boundary(&queued, &folders);
        db::discard_pending_sync_paths(&pool, &excluded_paths)
            .await
            .unwrap();

        let remaining = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].image.local_path.ends_with("Trips/keep.jpg"));
        let snapshot = db::get_sync_snapshot(&pool).await.unwrap();
        assert_eq!(
            (snapshot.total, snapshot.succeeded, snapshot.failed),
            (1, 0, 0)
        );
    }

    #[tokio::test]
    async fn enabled_resume_waiter_observes_a_concurrent_resume() {
        let runtime = SyncRuntime::new(test_pool().await, true);
        let waiter_runtime = runtime.clone();
        let waiting = tokio::spawn(async move { waiter_runtime.wait_while_paused().await });
        tokio::task::yield_now().await;

        runtime.resume_work();

        tokio::time::timeout(std::time::Duration::from_millis(50), waiting)
            .await
            .expect("an enabled waiter retains a concurrent resume notification")
            .unwrap();
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum RecordedEvent {
        QueuedWaiting,
        Started {
            status: db::SyncJobStatus,
            total: i64,
        },
        Paused {
            status: db::SyncJobStatus,
        },
        Resumed {
            status: db::SyncJobStatus,
        },
        Progressed {
            status: db::SyncJobStatus,
            total: i64,
            succeeded: i64,
            failed: i64,
        },
        Completed {
            status: db::SyncJobStatus,
            processed: usize,
            uploaded: usize,
            failed: usize,
        },
        Failed {
            status: Option<db::SyncJobStatus>,
            error: String,
        },
    }

    #[derive(Default)]
    struct RecordingReporter {
        events: std::sync::Mutex<Vec<RecordedEvent>>,
        paused_notify: Option<Arc<tokio::sync::Notify>>,
    }

    impl RecordingReporter {
        fn events(&self) -> Vec<RecordedEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl crate::sync::reporter::SyncRunReporter for RecordingReporter {
        fn queued_waiting(&self) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedEvent::QueuedWaiting);
        }

        fn started(&self, snapshot: &db::SyncSnapshot) {
            self.events.lock().unwrap().push(RecordedEvent::Started {
                status: snapshot.status,
                total: snapshot.total,
            });
        }

        fn paused(&self, snapshot: &db::SyncSnapshot) {
            self.events.lock().unwrap().push(RecordedEvent::Paused {
                status: snapshot.status,
            });
            if let Some(notify) = &self.paused_notify {
                notify.notify_waiters();
            }
        }

        fn resumed(&self, snapshot: &db::SyncSnapshot) {
            self.events.lock().unwrap().push(RecordedEvent::Resumed {
                status: snapshot.status,
            });
        }

        fn progressed(&self, snapshot: &db::SyncSnapshot) {
            self.events.lock().unwrap().push(RecordedEvent::Progressed {
                status: snapshot.status,
                total: snapshot.total,
                succeeded: snapshot.succeeded,
                failed: snapshot.failed,
            });
        }

        fn completed(&self, snapshot: &db::SyncSnapshot, totals: &SyncSummary) {
            self.events.lock().unwrap().push(RecordedEvent::Completed {
                status: snapshot.status,
                processed: totals.processed,
                uploaded: totals.uploaded,
                failed: totals.failed,
            });
        }

        fn failed(&self, snapshot: Option<&db::SyncSnapshot>, error: &str) {
            self.events.lock().unwrap().push(RecordedEvent::Failed {
                status: snapshot.map(|s| s.status),
                error: error.to_string(),
            });
        }
    }

    struct FakeBlockProcessor<F>(F);

    impl<F, Fut> QueueBlockProcessor for FakeBlockProcessor<F>
    where
        F: Fn(Vec<crate::media::model::UploadUnit>, crate::sync::ProcessingContext) -> Fut + Sync,
        Fut: std::future::Future<Output = Result<SyncSummary, String>> + Send,
    {
        async fn process_block(
            &self,
            units: Vec<crate::media::model::UploadUnit>,
            active_work: crate::sync::ProcessingContext,
        ) -> Result<SyncSummary, String> {
            (self.0)(units, active_work).await
        }
    }

    fn queue_asset(path: &str) -> db::QueueUploadUnit {
        db::QueueUploadUnit {
            image: db::QueueUploadAsset {
                path: path.to_string(),
                size: 100,
                mtime: 1,
                target_album_id: None,
            },
            live_photo_video: None,
        }
    }

    #[tokio::test]
    async fn queue_loop_reports_started_progressed_and_completed_for_full_run() {
        let pool = test_pool().await;
        db::enqueue_upload_units(
            &pool,
            &[
                queue_asset("C:/photos/one.jpg"),
                queue_asset("C:/photos/two.jpg"),
            ],
            &[],
        )
        .await
        .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.start_or_join().await.unwrap();

        let processor = FakeBlockProcessor(
            |units: Vec<UploadUnit>, _ctx: ProcessingContext| async move {
                let queue_results = units
                    .iter()
                    .map(|unit| (unit.image.path.clone(), "SYNCED".to_string()))
                    .collect::<Vec<_>>();
                let count = queue_results.len();
                Ok(SyncSummary {
                    processed: count,
                    uploaded: count,
                    failed: 0,
                    queue_results,
                })
            },
        );
        let reporter = RecordingReporter::default();

        let summary = run_queue_loop(&runtime, SyncSource::Manual, &processor, &reporter)
            .await
            .unwrap();

        assert_eq!(
            (summary.processed, summary.uploaded, summary.failed),
            (2, 2, 0)
        );
        assert!(!runtime.is_running());
        assert_eq!(
            reporter.events(),
            vec![
                RecordedEvent::Started {
                    status: db::SyncJobStatus::Running,
                    total: 2,
                },
                RecordedEvent::Progressed {
                    status: db::SyncJobStatus::Running,
                    total: 2,
                    succeeded: 2,
                    failed: 0,
                },
                RecordedEvent::Completed {
                    status: db::SyncJobStatus::Idle,
                    processed: 2,
                    uploaded: 2,
                    failed: 0,
                },
            ]
        );
    }

    #[tokio::test]
    async fn queue_loop_pauses_and_resumes_mid_run_keeping_remaining_work_pending() {
        let pool = test_pool().await;
        db::enqueue_upload_units(
            &pool,
            &[
                queue_asset("C:/photos/first.jpg"),
                queue_asset("C:/photos/second.jpg"),
                queue_asset("C:/photos/third.jpg"),
            ],
            &[],
        )
        .await
        .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.start_or_join().await.unwrap();

        let paused_notify = Arc::new(tokio::sync::Notify::new());
        let reporter = Arc::new(RecordingReporter {
            events: std::sync::Mutex::new(Vec::new()),
            paused_notify: Some(paused_notify.clone()),
        });

        struct PauseOnFirstBlockProcessor {
            runtime: SyncRuntime,
            calls: std::sync::atomic::AtomicUsize,
        }

        impl QueueBlockProcessor for PauseOnFirstBlockProcessor {
            async fn process_block(
                &self,
                units: Vec<UploadUnit>,
                _active_work: ProcessingContext,
            ) -> Result<SyncSummary, String> {
                let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call == 0 {
                    // Simulate pause requested mid-block: complete only the first item,
                    // leaving the rest pending in the durable queue.
                    self.runtime.pause().await.unwrap();
                    return Ok(SyncSummary {
                        processed: 1,
                        uploaded: 1,
                        failed: 0,
                        queue_results: vec![(units[0].image.path.clone(), "SYNCED".to_string())],
                    });
                }
                let queue_results = units
                    .iter()
                    .map(|unit| (unit.image.path.clone(), "SYNCED".to_string()))
                    .collect::<Vec<_>>();
                let count = queue_results.len();
                Ok(SyncSummary {
                    processed: count,
                    uploaded: count,
                    failed: 0,
                    queue_results,
                })
            }
        }

        let processor = PauseOnFirstBlockProcessor {
            runtime: runtime.clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };

        let loop_runtime = runtime.clone();
        let loop_reporter = reporter.clone();
        let handle = tokio::spawn(async move {
            run_queue_loop(
                &loop_runtime,
                SyncSource::Manual,
                &processor,
                loop_reporter.as_ref(),
            )
            .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(2), paused_notify.notified())
            .await
            .expect("queue loop must reach the pause boundary");

        let pending_while_paused = db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap();
        let pending_paths = pending_while_paused
            .iter()
            .map(|unit| unit.image.local_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            pending_paths,
            vec!["C:/photos/second.jpg", "C:/photos/third.jpg"]
        );

        runtime.resume().await.unwrap();

        let summary = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("queue loop must complete after resume")
            .unwrap()
            .unwrap();

        assert_eq!(
            (summary.processed, summary.uploaded, summary.failed),
            (3, 3, 0)
        );
        assert!(db::next_queued_upload_units(&pool, SYNC_QUEUE_BLOCK_SIZE)
            .await
            .unwrap()
            .is_empty());
        assert!(!runtime.is_running());
        assert_eq!(
            reporter.events(),
            vec![
                RecordedEvent::Started {
                    status: db::SyncJobStatus::Running,
                    total: 3,
                },
                RecordedEvent::Progressed {
                    status: db::SyncJobStatus::Paused,
                    total: 3,
                    succeeded: 1,
                    failed: 0,
                },
                RecordedEvent::Paused {
                    status: db::SyncJobStatus::Paused,
                },
                RecordedEvent::Resumed {
                    status: db::SyncJobStatus::Running,
                },
                RecordedEvent::Progressed {
                    status: db::SyncJobStatus::Running,
                    total: 3,
                    succeeded: 3,
                    failed: 0,
                },
                RecordedEvent::Completed {
                    status: db::SyncJobStatus::Idle,
                    processed: 3,
                    uploaded: 3,
                    failed: 0,
                },
            ]
        );
    }

    #[tokio::test]
    async fn queue_loop_discards_excluded_paths_at_queue_boundary_and_reports_progressed() {
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("lymic-loop-excl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("Screenshots")).unwrap();
        std::fs::create_dir_all(root.join("Trips")).unwrap();
        let folder_id = db::add_folder(&pool, &root.to_string_lossy(), None)
            .await
            .unwrap();
        db::update_exclusion_patterns(&pool, folder_id, &["screenshots/**".to_string()])
            .await
            .unwrap();

        let excluded = root.join("Screenshots/private.jpg");
        let included = root.join("Trips/keep.jpg");
        db::enqueue_upload_units(
            &pool,
            &[
                queue_asset(&excluded.to_string_lossy()),
                queue_asset(&included.to_string_lossy()),
            ],
            &[],
        )
        .await
        .unwrap();

        let runtime = SyncRuntime::new(pool, false);
        runtime.start_or_join().await.unwrap();

        let processor = FakeBlockProcessor(
            |units: Vec<UploadUnit>, _ctx: ProcessingContext| async move {
                let queue_results = units
                    .iter()
                    .map(|unit| (unit.image.path.clone(), "SYNCED".to_string()))
                    .collect::<Vec<_>>();
                let count = queue_results.len();
                Ok(SyncSummary {
                    processed: count,
                    uploaded: count,
                    failed: 0,
                    queue_results,
                })
            },
        );
        let reporter = RecordingReporter::default();

        let summary = run_queue_loop(&runtime, SyncSource::Manual, &processor, &reporter)
            .await
            .unwrap();

        assert_eq!(
            (summary.processed, summary.uploaded, summary.failed),
            (1, 1, 0)
        );
        assert_eq!(
            reporter.events(),
            vec![
                RecordedEvent::Started {
                    status: db::SyncJobStatus::Running,
                    total: 2,
                },
                RecordedEvent::Progressed {
                    status: db::SyncJobStatus::Running,
                    total: 1,
                    succeeded: 0,
                    failed: 0,
                },
                RecordedEvent::Progressed {
                    status: db::SyncJobStatus::Running,
                    total: 1,
                    succeeded: 1,
                    failed: 0,
                },
                RecordedEvent::Completed {
                    status: db::SyncJobStatus::Idle,
                    processed: 1,
                    uploaded: 1,
                    failed: 0,
                },
            ]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn queue_loop_reports_failed_and_persists_failed_status_on_processor_error() {
        let pool = test_pool().await;
        db::enqueue_upload_units(&pool, &[queue_asset("C:/photos/one.jpg")], &[])
            .await
            .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.start_or_join().await.unwrap();

        let processor = FakeBlockProcessor(
            |_units: Vec<UploadUnit>, _ctx: ProcessingContext| async move {
                Err("immich unreachable".to_string())
            },
        );
        let reporter = RecordingReporter::default();

        let err = run_queue_loop(&runtime, SyncSource::Manual, &processor, &reporter)
            .await
            .unwrap_err();
        assert_eq!(err, "immich unreachable");
        assert!(!runtime.is_running());
        assert_eq!(
            db::get_sync_snapshot(&pool).await.unwrap().status,
            db::SyncJobStatus::Failed
        );
        assert_eq!(
            reporter.events(),
            vec![
                RecordedEvent::Started {
                    status: db::SyncJobStatus::Running,
                    total: 1,
                },
                RecordedEvent::Failed {
                    status: Some(db::SyncJobStatus::Failed),
                    error: "immich unreachable".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn queue_loop_reports_started_before_failing_when_processor_prepare_fails() {
        let pool = test_pool().await;
        db::enqueue_upload_units(&pool, &[queue_asset("C:/photos/one.jpg")], &[])
            .await
            .unwrap();
        let runtime = SyncRuntime::new(pool.clone(), false);
        runtime.start_or_join().await.unwrap();

        struct FailingPrepareProcessor;
        impl QueueBlockProcessor for FailingPrepareProcessor {
            async fn prepare(&self) -> Result<(), String> {
                Err("could not load settings".to_string())
            }

            async fn process_block(
                &self,
                _units: Vec<UploadUnit>,
                _active_work: ProcessingContext,
            ) -> Result<SyncSummary, String> {
                unreachable!("process_block must not run when prepare fails")
            }
        }

        let reporter = RecordingReporter::default();
        let err = run_queue_loop(&runtime, SyncSource::Manual, &FailingPrepareProcessor, &reporter)
            .await
            .unwrap_err();
        assert_eq!(err, "could not load settings");
        assert!(!runtime.is_running());
        assert_eq!(
            reporter.events(),
            vec![
                RecordedEvent::Started {
                    status: db::SyncJobStatus::Running,
                    total: 1,
                },
                RecordedEvent::Failed {
                    status: Some(db::SyncJobStatus::Failed),
                    error: "could not load settings".to_string(),
                },
            ]
        );
    }
}
