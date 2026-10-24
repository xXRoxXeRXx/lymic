use crate::{
    db::WatchedFolder,
    sync::{add_event_paths, scan_watched_folders_for_media, Asset, FileFailure, ScanResult},
};
use std::{collections::HashMap, time::Duration};
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
}

impl WatcherIngestion {
    pub(crate) fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            files: HashMap::new(),
            failures: HashMap::new(),
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
    use std::path::PathBuf;
    use tokio::sync::mpsc;

    #[tokio::test(start_paused = true)]
    async fn rapid_events_produce_one_consolidated_scan_after_the_debounce() {
        let (tx, mut rx) = mpsc::channel(4);
        let first = notify::Event::new(notify::EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("first.jpg"));
        let second = notify::Event::new(notify::EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("second.jpg"));
        tx.send(WatcherInput::Event(first)).await.unwrap();
        tx.send(WatcherInput::Event(second)).await.unwrap();

        let mut ingestion = WatcherIngestion::new(WATCHER_DEBOUNCE);
        let batch = tokio::spawn(async move { ingestion.next_batch(&mut rx).await.unwrap() });

        tokio::task::yield_now().await;
        assert!(!batch.is_finished());
        tokio::time::advance(WATCHER_DEBOUNCE).await;
        let batch = batch.await.unwrap();

        assert_eq!(batch.events.len(), 2);
        assert!(!batch.requires_full_rescan);
    }

    #[tokio::test]
    async fn overflow_discards_partial_event_state_and_rescans_every_watched_folder() {
        let root = std::env::temp_dir().join(format!("lymic-ingestion-{}", uuid::Uuid::new_v4()));
        let photo = root.join("complete.jpg");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&photo, b"media").unwrap();
        let folders = vec![crate::db::WatchedFolder {
            id: 1,
            path: root.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec![],
        }];
        let mut ingestion = WatcherIngestion::new(Duration::ZERO);
        let result = ingestion
            .scan(
                &folders,
                WatcherBatch {
                    events: vec![],
                    requires_full_rescan: true,
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, photo.to_string_lossy());
        std::fs::remove_dir_all(root).unwrap();
    }
}
