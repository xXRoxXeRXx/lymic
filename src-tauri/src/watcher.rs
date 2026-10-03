use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Notify;

pub(crate) fn send_or_request_rescan<T>(
    tx: &mpsc::Sender<T>,
    message: T,
    rescan_requested: &AtomicBool,
    rescan_notify: &Notify,
) {
    if tx.try_send(message).is_err() {
        request_rescan(rescan_requested, rescan_notify);
    }
}

pub(crate) fn request_rescan(rescan_requested: &AtomicBool, rescan_notify: &Notify) {
    rescan_requested.store(true, Ordering::Release);
    // Notify retains a permit when the receiver is idle, so a late overflow
    // cannot leave a dirty flag without a future reconciliation trigger.
    rescan_notify.notify_one();
}

pub fn create_watcher(
    tx: mpsc::Sender<Event>,
    rescan_requested: Arc<AtomicBool>,
    rescan_notify: Arc<Notify>,
) -> notify::Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        // A full queue means individual paths are no longer trustworthy. The
                        // receiver will reconcile every watched folder after draining its batch.
                        send_or_request_rescan(&tx, event, &rescan_requested, &rescan_notify);
                    }
                    _ => {}
                },
                Err(_) => {
                    // notify itself can report an overflow or backend error. Reconcile rather
                    // than silently accepting a potentially incomplete event stream.
                    request_rescan(&rescan_requested, &rescan_notify);
                }
            }
        },
        Config::default(),
    )?;

    Ok(watcher)
}

pub struct WatcherState {
    watcher: RecommendedWatcher,
    watched_paths: HashSet<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WatchRegistration {
    Registered,
    AlreadyWatched,
    PathUnavailable,
}

impl WatcherState {
    pub fn new(watcher: RecommendedWatcher) -> Self {
        Self {
            watcher,
            watched_paths: HashSet::new(),
        }
    }

    pub fn watch_path(&mut self, path: &str) -> notify::Result<WatchRegistration> {
        self.reconcile_path(path, Path::new(path).exists())
    }

    pub fn reconcile_path(
        &mut self,
        path: impl AsRef<Path>,
        is_available: bool,
    ) -> notify::Result<WatchRegistration> {
        let path = path.as_ref().to_path_buf();
        if !is_available {
            self.stop_watching(&path)?;
            return Ok(WatchRegistration::PathUnavailable);
        }

        if self.watched_paths.contains(&path) {
            return Ok(WatchRegistration::AlreadyWatched);
        }

        self.watcher.watch(&path, RecursiveMode::Recursive)?;
        self.watched_paths.insert(path);
        Ok(WatchRegistration::Registered)
    }

    pub fn unwatch_path(&mut self, path: &str) -> notify::Result<()> {
        self.stop_watching(&PathBuf::from(path))
    }

    fn stop_watching(&mut self, path: &Path) -> notify::Result<()> {
        if !self.watched_paths.remove(path) {
            return Ok(());
        }

        if let Err(error) = self.watcher.unwatch(path) {
            if !matches!(&error.kind, notify::ErrorKind::WatchNotFound) {
                return Err(error);
            }
        }
        Ok(())
    }
}

pub fn watch_path(watcher: &mut WatcherState, path: &str) -> notify::Result<WatchRegistration> {
    watcher.watch_path(path)
}

pub fn unwatch_path(watcher: &mut WatcherState, path: &str) -> notify::Result<()> {
    watcher.unwatch_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_watcher() -> WatcherState {
        let (tx, _rx) = mpsc::channel(1);
        WatcherState::new(
            create_watcher(
                tx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Notify::new()),
            )
            .unwrap(),
        )
    }

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-watcher-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[tokio::test]
    async fn full_channel_requests_a_rescan() {
        let (tx, _rx) = mpsc::channel(1);
        let dirty = AtomicBool::new(false);
        let notify = Notify::new();

        tx.try_send(1).unwrap();
        send_or_request_rescan(&tx, 2, &dirty, &notify);

        assert!(dirty.load(Ordering::Acquire));
        assert!(
            tokio::time::timeout(std::time::Duration::ZERO, notify.notified())
                .await
                .is_ok()
        );
    }

    #[test]
    fn registers_existing_directory_once() {
        let path = temporary_directory();
        let mut watcher = test_watcher();

        assert_eq!(
            watcher.watch_path(path.to_str().unwrap()).unwrap(),
            WatchRegistration::Registered
        );
        assert_eq!(
            watcher.watch_path(path.to_str().unwrap()).unwrap(),
            WatchRegistration::AlreadyWatched
        );

        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn unavailable_directory_is_not_watched_and_can_be_unwatched() {
        let path =
            std::env::temp_dir().join(format!("lymic-watcher-missing-{}", std::process::id()));
        let mut watcher = test_watcher();

        assert_eq!(
            watcher.watch_path(path.to_str().unwrap()).unwrap(),
            WatchRegistration::PathUnavailable
        );
        assert!(watcher.unwatch_path(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn unavailable_directory_is_reregistered_when_it_returns() {
        let path = temporary_directory();
        let path_string = path.to_str().unwrap().to_owned();
        let mut watcher = test_watcher();

        assert_eq!(
            watcher.reconcile_path(&path_string, true).unwrap(),
            WatchRegistration::Registered
        );
        std::fs::remove_dir(&path).unwrap();
        assert_eq!(
            watcher.reconcile_path(&path_string, false).unwrap(),
            WatchRegistration::PathUnavailable
        );
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            watcher.reconcile_path(&path_string, true).unwrap(),
            WatchRegistration::Registered
        );

        std::fs::remove_dir(path).unwrap();
    }
}
