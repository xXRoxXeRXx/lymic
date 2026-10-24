use crate::media::paths::{normalize_folder_path, path_is_within};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio_util::sync::CancellationToken;

/// Coordinates cancellation for the bounded queue blocks currently in flight.
pub(crate) struct ActiveWork {
    entries: Mutex<Vec<ActiveEntry>>,
    next_id: AtomicU64,
}

struct ActiveEntry {
    id: u64,
    path: PathBuf,
    cancellation: CancellationToken,
}

/// Registration and processing context for one queue block.
///
/// Dropping the lease unregisters all of its paths.
pub(crate) struct ActiveWorkLease {
    work: Arc<ActiveWork>,
    ids: Vec<u64>,
    context: ProcessingContext,
}

#[derive(Clone)]
pub(crate) struct ProcessingContext {
    entries: Vec<ContextEntry>,
}

#[derive(Clone)]
struct ContextEntry {
    path: PathBuf,
    cancellation: CancellationToken,
}

impl ActiveWork {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn acquire(
        self: &Arc<Self>,
        paths: impl IntoIterator<Item = String>,
    ) -> ActiveWorkLease {
        let mut entries = self.entries.lock().expect("active work lock poisoned");
        let mut ids = Vec::new();
        let mut context_entries = Vec::new();
        for path in paths {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let cancellation = CancellationToken::new();
            let path = normalize_folder_path(&PathBuf::from(path));
            entries.push(ActiveEntry {
                id,
                path: path.clone(),
                cancellation: cancellation.clone(),
            });
            ids.push(id);
            context_entries.push(ContextEntry { path, cancellation });
        }
        ActiveWorkLease {
            work: self.clone(),
            ids,
            context: ProcessingContext {
                entries: context_entries,
            },
        }
    }

    pub(crate) fn has_work_in(&self, folder: &str) -> bool {
        let folder = normalize_folder_path(&PathBuf::from(folder));
        self.entries
            .lock()
            .expect("active work lock poisoned")
            .iter()
            .any(|entry| path_is_within(&entry.path, &folder))
    }

    pub(crate) fn cancel_work_in(&self, folder: &str) {
        let folder = normalize_folder_path(&PathBuf::from(folder));
        for entry in self
            .entries
            .lock()
            .expect("active work lock poisoned")
            .iter()
        {
            if path_is_within(&entry.path, &folder) {
                entry.cancellation.cancel();
            }
        }
    }
}

impl ActiveWorkLease {
    pub(crate) fn context(&self) -> ProcessingContext {
        self.context.clone()
    }
}

impl Drop for ActiveWorkLease {
    fn drop(&mut self) {
        self.work
            .entries
            .lock()
            .expect("active work lock poisoned")
            .retain(|entry| !self.ids.contains(&entry.id));
    }
}

impl ProcessingContext {
    pub(crate) fn cancellation_for_path(&self, path: &str) -> Option<CancellationToken> {
        let path = normalize_folder_path(&PathBuf::from(path));
        self.entries.iter().find_map(|entry| {
            (path_is_within(&entry.path, &path) && path_is_within(&path, &entry.path))
                .then(|| entry.cancellation.clone())
        })
    }

    pub(crate) fn is_path_cancelled(&self, path: &str) -> bool {
        self.cancellation_for_path(path)
            .is_some_and(|cancellation| cancellation.is_cancelled())
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveWork;
    use std::sync::Arc;

    #[test]
    fn cancelling_one_folder_does_not_cancel_another() {
        let work = Arc::new(ActiveWork::new());
        let photos = work.acquire(["C:/Photos/active.jpg".to_string()]);
        let videos = work.acquire(["C:/Videos/active.jpg".to_string()]);

        work.cancel_work_in("C:/Photos");

        assert!(photos.context().is_path_cancelled("C:/Photos/active.jpg"));
        assert!(!videos.context().is_path_cancelled("C:/Videos/active.jpg"));
    }

    #[test]
    fn ending_a_lease_removes_its_registration() {
        let work = Arc::new(ActiveWork::new());
        let lease = work.acquire(["C:/Photos/active.jpg".to_string()]);
        assert!(work.has_work_in("C:/Photos"));

        drop(lease);

        assert!(!work.has_work_in("C:/Photos"));
    }
}
