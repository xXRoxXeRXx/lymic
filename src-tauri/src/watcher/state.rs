use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
        self.reconcile_path(path, Path::new(path).is_dir())
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
        if !self.watched_paths.contains(path) {
            return Ok(());
        }

        if let Err(error) = self.watcher.unwatch(path) {
            if !matches!(&error.kind, notify::ErrorKind::WatchNotFound) {
                return Err(error);
            }
        }
        self.watched_paths.remove(path);
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
    use crate::watcher::create_watcher;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Notify};

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
    fn file_is_not_registered_as_a_directory() {
        let path = std::env::temp_dir().join(format!(
            "lymic-watcher-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "not a directory").unwrap();
        let mut watcher = test_watcher();
        assert_eq!(
            watcher.watch_path(path.to_str().unwrap()).unwrap(),
            WatchRegistration::PathUnavailable
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unwatching_allows_directory_to_be_registered_again() {
        let path = temporary_directory();
        let path_string = path.to_str().unwrap();
        let mut watcher = test_watcher();
        assert_eq!(
            watcher.watch_path(path_string).unwrap(),
            WatchRegistration::Registered
        );
        watcher.unwatch_path(path_string).unwrap();
        assert_eq!(
            watcher.watch_path(path_string).unwrap(),
            WatchRegistration::Registered
        );
        std::fs::remove_dir(path).unwrap();
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
