use crate::{
    db::WatchedFolder,
    sync::{add_event_paths, scan_watched_folders_for_media, Asset, FileFailure, ScanResult},
};
use std::{
    collections::HashMap,
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::{mpsc, Notify};

pub(crate) const WATCHER_DEBOUNCE: Duration = Duration::from_secs(2);

/// The narrow boundary between the native watcher and watcher ingestion.
#[derive(Debug)]
pub(crate) enum WatcherInput {
    Event(notify::Event),
    Overflow,
}

#[derive(Debug)]
pub(crate) struct WatcherBatch {
    pub(crate) events: Vec<notify::Event>,
    pub(crate) requires_full_rescan: bool,
}

/// Consolidates native watcher inputs into one scan result per quiet period.
///
/// It owns debounce and the event buffer. Media discovery and exclusion
/// admission remain at their existing seams.
pub(crate) struct WatcherIngestion {
    debounce: Duration,
    files: HashMap<String, Asset>,
    failures: HashMap<String, FileFailure>,
    pending_batch: Option<WatcherBatch>,
}

impl WatcherIngestion {
    pub(crate) fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            files: HashMap::new(),
            failures: HashMap::new(),
            pending_batch: None,
        }
    }

    pub(crate) async fn next_batch(
        &mut self,
        receiver: &mut mpsc::Receiver<WatcherInput>,
        rescan_requested: &AtomicBool,
        rescan_notify: &Notify,
    ) -> Option<WatcherBatch> {
        let mut events = Vec::new();
        let mut requires_full_rescan = rescan_requested.swap(false, Ordering::AcqRel);
        while !requires_full_rescan && events.is_empty() {
            tokio::select! {
                input = receiver.recv() => Self::record(input?, &mut events, &mut requires_full_rescan),
                _ = rescan_notify.notified() => {
                    requires_full_rescan = rescan_requested.swap(false, Ordering::AcqRel);
                }
            }
        }
        if requires_full_rescan {
            return Some(WatcherBatch {
                events,
                requires_full_rescan,
            });
        }
        let debounce = tokio::time::sleep(self.debounce);
        tokio::pin!(debounce);
        loop {
            tokio::select! {
                input = receiver.recv() => match input {
                    Some(input) => {
                        Self::record(input, &mut events, &mut requires_full_rescan);
                        debounce.as_mut().reset(tokio::time::Instant::now() + self.debounce);
                    }
                    None => break,
                },
                _ = rescan_notify.notified() => {
                    if rescan_requested.swap(false, Ordering::AcqRel) {
                        requires_full_rescan = true;
                        break;
                    }
                },
                _ = &mut debounce => break,
            }
        }
        Some(WatcherBatch {
            events,
            requires_full_rescan,
        })
    }

    /// Resolves Watched Folders, scans one consolidated native event sequence,
    /// then hands its result to Synchronization. A resolution failure retains
    /// the consumed batch so ambiguous native events are reconciled on retry.
    pub(crate) async fn ingest_next<Resolve, ResolveFuture, Handoff, HandoffFuture>(
        &mut self,
        receiver: &mut mpsc::Receiver<WatcherInput>,
        rescan_requested: &AtomicBool,
        rescan_notify: &Notify,
        resolve_folders: Resolve,
        handoff: Handoff,
    ) -> Result<Option<bool>, String>
    where
        Resolve: FnOnce() -> ResolveFuture,
        ResolveFuture: Future<Output = Result<Vec<WatchedFolder>, String>>,
        Handoff: FnOnce(ScanResult) -> HandoffFuture,
        HandoffFuture: Future<Output = Result<(), String>>,
    {
        let batch = match self.pending_batch.take() {
            Some(batch) => batch,
            None => match self
                .next_batch(receiver, rescan_requested, rescan_notify)
                .await
            {
                Some(batch) => batch,
                None => return Ok(None),
            },
        };
        let requires_full_rescan = batch.requires_full_rescan;
        let folders = match resolve_folders().await {
            Ok(folders) => folders,
            Err(error) => {
                self.pending_batch = Some(batch);
                return Err(error);
            }
        };
        let scan_result = match self.scan(&folders, &batch).await {
            Ok(scan_result) => scan_result,
            Err(error) => {
                self.pending_batch = Some(batch);
                return Err(error);
            }
        };
        if let Some(scan_result) = scan_result {
            if let Err(error) = handoff(scan_result).await {
                self.pending_batch = Some(batch);
                return Err(error);
            }
        }
        Ok(Some(requires_full_rescan))
    }

    pub(crate) fn has_pending_batch(&self) -> bool {
        self.pending_batch.is_some()
    }

    fn record(input: WatcherInput, events: &mut Vec<notify::Event>, full_rescan: &mut bool) {
        match input {
            WatcherInput::Event(event) => events.push(event),
            WatcherInput::Overflow => *full_rescan = true,
        }
    }

    pub(crate) async fn scan(
        &mut self,
        folders: &[WatchedFolder],
        batch: &WatcherBatch,
    ) -> Result<Option<ScanResult>, String> {
        if batch.requires_full_rescan {
            self.files.clear();
            self.failures.clear();
            return scan_watched_folders_for_media(folders.to_vec())
                .await
                .map(Some);
        }
        for event in &batch.events {
            add_event_paths(&mut self.files, &mut self.failures, folders, event.clone()).await?;
        }
        if self.files.is_empty() && self.failures.is_empty() {
            return Ok(None);
        }
        let mut result = ScanResult::default();
        result
            .files
            .extend(self.files.drain().map(|(_, asset)| asset));
        result
            .failures
            .extend(self.failures.drain().map(|(_, failure)| failure));
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::CreateKind;
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::sync::mpsc;

    #[tokio::test(start_paused = true)]
    async fn rapid_events_produce_one_scan_and_synchronization_handoff() {
        let root = test_folder_with_media("first.jpg");
        let folders = folders_for(&root);
        let (tx, mut rx) = mpsc::channel(4);
        let first = notify::Event::new(notify::EventKind::Create(CreateKind::File))
            .add_path(root.join("first.jpg"));
        let second = notify::Event::new(notify::EventKind::Create(CreateKind::File))
            .add_path(root.join("first.jpg"));
        tx.send(WatcherInput::Event(first)).await.unwrap();
        tx.send(WatcherInput::Event(second)).await.unwrap();

        let mut ingestion = WatcherIngestion::new(WATCHER_DEBOUNCE);
        let rescan_requested = Arc::new(AtomicBool::new(false));
        let rescan_notify = Arc::new(Notify::new());
        let handoffs = Arc::new(AtomicUsize::new(0));
        let observed_files = Arc::new(AtomicUsize::new(0));
        let handoffs_for_task = handoffs.clone();
        let observed_for_task = observed_files.clone();
        let rescan_requested_for_task = rescan_requested.clone();
        let rescan_notify_for_task = rescan_notify.clone();
        let batch = tokio::spawn(async move {
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested_for_task,
                    &rescan_notify_for_task,
                    || async move { Ok(folders) },
                    move |result| async move {
                        observed_for_task.store(result.files.len(), Ordering::SeqCst);
                        handoffs_for_task.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
                .unwrap()
        });

        tokio::task::yield_now().await;
        assert!(!batch.is_finished());
        tokio::time::advance(WATCHER_DEBOUNCE).await;
        assert_eq!(batch.await.unwrap(), Some(false));

        assert_eq!(handoffs.load(Ordering::SeqCst), 1);
        assert_eq!(observed_files.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn queue_full_fallback_during_debounce_discards_partial_batch_without_another_event() {
        let root = test_folder_with_media("complete.jpg");
        let complete = root.join("complete.jpg").to_string_lossy().to_string();
        let folders = folders_for(&root);
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(WatcherInput::Event(
            notify::Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("partial.jpg")),
        ))
        .await
        .unwrap();
        let mut ingestion = WatcherIngestion::new(WATCHER_DEBOUNCE);
        let rescan_requested = Arc::new(AtomicBool::new(false));
        let rescan_notify = Arc::new(Notify::new());
        let requested_for_task = rescan_requested.clone();
        let notify_for_task = rescan_notify.clone();
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_task = observed.clone();
        let task = tokio::spawn(async move {
            ingestion
                .ingest_next(
                    &mut rx,
                    &requested_for_task,
                    &notify_for_task,
                    || async move { Ok(folders) },
                    move |result| async move {
                        *observed_for_task.lock().unwrap() =
                            result.files.into_iter().map(|file| file.path).collect();
                        Ok(())
                    },
                )
                .await
                .unwrap()
        });

        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        super::super::callback::request_rescan(&rescan_requested, &rescan_notify);
        assert_eq!(task.await.unwrap(), Some(true));
        assert_eq!(*observed.lock().unwrap(), vec![complete]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn overflow_discards_partial_events_and_hands_off_a_full_rescan() {
        let root = test_folder_with_media("complete.jpg");
        let photo = root.join("complete.jpg");
        let folders = folders_for(&root);
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(WatcherInput::Event(
            notify::Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("partial.txt")),
        ))
        .await
        .unwrap();
        tx.send(WatcherInput::Overflow).await.unwrap();
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let rescan_requested = AtomicBool::new(false);
        let rescan_notify = Notify::new();
        let scanned_paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = scanned_paths.clone();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async move { Ok(folders) },
                    move |result| async move {
                        *observed.lock().unwrap() =
                            result.files.into_iter().map(|file| file.path).collect();
                        Ok(())
                    }
                )
                .await
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            *scanned_paths.lock().unwrap(),
            vec![photo.to_string_lossy().to_string()]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn irrelevant_events_produce_no_synchronization_work() {
        let root = test_folder_with_media("photo.jpg");
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(WatcherInput::Event(
            notify::Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("notes.txt")),
        ))
        .await
        .unwrap();
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let rescan_requested = AtomicBool::new(false);
        let rescan_notify = Notify::new();
        let handoffs = Arc::new(AtomicUsize::new(0));
        let observed = handoffs.clone();
        let folder_root = root.clone();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async move { Ok(folders_for(&folder_root)) },
                    move |_| async move {
                        observed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                )
                .await
                .unwrap(),
            Some(false)
        );
        assert_eq!(handoffs.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn resolution_failure_retains_the_consumed_batch_for_reconciliation() {
        let root = test_folder_with_media("photo.jpg");
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(WatcherInput::Event(
            notify::Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("photo.jpg")),
        ))
        .await
        .unwrap();
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let rescan_requested = AtomicBool::new(false);
        let rescan_notify = Notify::new();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async { Err("temporary database failure".to_string()) },
                    |_| async { Ok(()) }
                )
                .await
                .unwrap_err(),
            "temporary database failure"
        );
        let handoffs = Arc::new(AtomicUsize::new(0));
        let observed = handoffs.clone();
        let folder_root = root.clone();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async move { Ok(folders_for(&folder_root)) },
                    move |result| async move {
                        assert_eq!(result.files.len(), 1);
                        observed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                )
                .await
                .unwrap(),
            Some(false)
        );
        assert_eq!(handoffs.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn admission_failure_retains_the_consolidated_batch_for_retry() {
        let root = test_folder_with_media("photo.jpg");
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(WatcherInput::Event(
            notify::Event::new(notify::EventKind::Create(CreateKind::File))
                .add_path(root.join("photo.jpg")),
        ))
        .await
        .unwrap();
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let rescan_requested = AtomicBool::new(false);
        let rescan_notify = Notify::new();
        let folder_root = root.clone();

        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async move { Ok(folders_for(&folder_root)) },
                    |_| async { Err("temporary admission failure".to_string()) },
                )
                .await
                .unwrap_err(),
            "temporary admission failure"
        );

        let folder_root = root.clone();
        let retry = tokio::time::timeout(
            Duration::from_millis(50),
            ingestion.ingest_next(
                &mut rx,
                &rescan_requested,
                &rescan_notify,
                || async move { Ok(folders_for(&folder_root)) },
                |result| async move {
                    assert_eq!(result.files.len(), 1);
                    Ok(())
                },
            ),
        )
        .await;

        assert!(matches!(retry, Ok(Ok(Some(false)))));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn admission_failure_retains_the_full_rescan_requirement_for_retry() {
        let root = test_folder_with_media("photo.jpg");
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(WatcherInput::Overflow).await.unwrap();
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let rescan_requested = AtomicBool::new(false);
        let rescan_notify = Notify::new();
        let folder_root = root.clone();

        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    &rescan_requested,
                    &rescan_notify,
                    || async move { Ok(folders_for(&folder_root)) },
                    |_| async { Err("temporary admission failure".to_string()) },
                )
                .await
                .unwrap_err(),
            "temporary admission failure"
        );

        let folder_root = root.clone();
        let retry = tokio::time::timeout(
            Duration::from_millis(50),
            ingestion.ingest_next(
                &mut rx,
                &rescan_requested,
                &rescan_notify,
                || async move { Ok(folders_for(&folder_root)) },
                |result| async move {
                    assert_eq!(result.files.len(), 1);
                    Ok(())
                },
            ),
        )
        .await;

        assert!(matches!(retry, Ok(Ok(Some(true)))));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_folder_with_media(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lymic-ingestion-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(name), b"media").unwrap();
        crate::media::paths::canonicalize_folder_path(&root)
    }

    fn folders_for(root: &std::path::Path) -> Vec<crate::db::WatchedFolder> {
        vec![crate::db::WatchedFolder {
            id: 1,
            path: root.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec![],
        }]
    }
}
