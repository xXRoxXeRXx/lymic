use crate::{
    db::WatchedFolder,
    sync::{add_event_paths, scan_watched_folders_for_media, Asset, FileFailure, ScanResult},
};
use std::{collections::HashMap, future::Future, time::Duration};
use tokio::sync::mpsc;

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
    ) -> Option<WatcherBatch> {
        let first = receiver.recv().await?;
        let mut events = Vec::new();
        let mut requires_full_rescan = false;
        Self::record(first, &mut events, &mut requires_full_rescan);
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
        force_full_rescan: bool,
        resolve_folders: Resolve,
        handoff: Handoff,
    ) -> Result<Option<bool>, String>
    where
        Resolve: FnOnce() -> ResolveFuture,
        ResolveFuture: Future<Output = Result<Vec<WatchedFolder>, String>>,
        Handoff: FnOnce(ScanResult) -> HandoffFuture,
        HandoffFuture: Future<Output = Result<(), String>>,
    {
        let mut batch = match self.pending_batch.take() {
            Some(batch) => batch,
            None => match self.next_batch(receiver).await {
                Some(batch) => batch,
                None => return Ok(None),
            },
        };
        batch.requires_full_rescan |= force_full_rescan;
        let requires_full_rescan = batch.requires_full_rescan;
        let folders = match resolve_folders().await {
            Ok(folders) => folders,
            Err(error) => {
                self.pending_batch = Some(batch);
                return Err(error);
            }
        };
        if let Some(scan_result) = self.scan(&folders, batch).await? {
            handoff(scan_result).await?;
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
        batch: WatcherBatch,
    ) -> Result<Option<ScanResult>, String> {
        if batch.requires_full_rescan {
            self.files.clear();
            self.failures.clear();
            return scan_watched_folders_for_media(folders.to_vec())
                .await
                .map(Some);
        }
        for event in batch.events {
            add_event_paths(&mut self.files, &mut self.failures, folders, event).await?;
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
            atomic::{AtomicUsize, Ordering},
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
        let handoffs = Arc::new(AtomicUsize::new(0));
        let observed_files = Arc::new(AtomicUsize::new(0));
        let handoffs_for_task = handoffs.clone();
        let observed_for_task = observed_files.clone();
        let batch = tokio::spawn(async move {
            ingestion
                .ingest_next(
                    &mut rx,
                    false,
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
        let scanned_paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = scanned_paths.clone();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    false,
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
        let handoffs = Arc::new(AtomicUsize::new(0));
        let observed = handoffs.clone();
        let folder_root = root.clone();
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    false,
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
        assert_eq!(
            ingestion
                .ingest_next(
                    &mut rx,
                    false,
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
                    false,
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

    fn test_folder_with_media(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lymic-ingestion-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(name), b"media").unwrap();
        root
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
