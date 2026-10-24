use crate::media::{
    model::{HashedUploadUnit, UploadUnit},
    paths::{normalize_folder_path, path_is_within},
};
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

#[derive(Clone)]
pub(crate) struct UnitProcessingContext {
    cancellation: CancellationToken,
}

impl ActiveWork {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn acquire(self: &Arc<Self>, units: &[UploadUnit]) -> ActiveWorkLease {
        let mut entries = self.entries.lock().expect("active work lock poisoned");
        let mut ids = Vec::new();
        let mut context_entries = Vec::new();
        for path in units
            .iter()
            .flat_map(UploadUnit::assets)
            .map(|asset| asset.path.clone())
        {
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
    pub(crate) fn for_upload_unit(&self, unit: &UploadUnit) -> UnitProcessingContext {
        self.for_path(&unit.image.path)
    }

    pub(crate) fn for_hashed_upload_unit(&self, unit: &HashedUploadUnit) -> UnitProcessingContext {
        self.for_path(&unit.image.path)
    }

    fn for_path(&self, path: &str) -> UnitProcessingContext {
        let path = normalize_folder_path(&PathBuf::from(path));
        let cancellation = self.entries.iter().find_map(|entry| {
            (path_is_within(&entry.path, &path) && path_is_within(&path, &entry.path))
                .then(|| entry.cancellation.clone())
        });
        UnitProcessingContext {
            cancellation: cancellation
                .expect("lease context must contain every queued Upload Unit"),
        }
    }
}

impl UnitProcessingContext {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub(crate) fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveWork;
    use crate::media::model::{Asset, UploadUnit};
    use std::sync::Arc;

    #[test]
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

    #[test]
    fn cancelling_one_folder_does_not_cancel_another() {
        let work = Arc::new(ActiveWork::new());
        let photos_unit = unit("C:/Photos/active.jpg");
        let videos_unit = unit("C:/Videos/active.jpg");
        let photos = work.acquire(&[photos_unit.clone()]);
        let videos = work.acquire(&[videos_unit.clone()]);

        work.cancel_work_in("C:/Photos");

        assert!(photos
            .context()
            .for_upload_unit(&photos_unit)
            .is_cancelled());
        assert!(!videos
            .context()
            .for_upload_unit(&videos_unit)
            .is_cancelled());
    }

    #[test]
    fn ending_a_lease_removes_its_registration() {
        let work = Arc::new(ActiveWork::new());
        let unit = unit("C:/Photos/active.jpg");
        let lease = work.acquire(&[unit]);
        assert!(work.has_work_in("C:/Photos"));

        drop(lease);

        assert!(!work.has_work_in("C:/Photos"));
    }
}
