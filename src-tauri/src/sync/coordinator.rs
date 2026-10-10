use crate::media::paths::{normalize_folder_path, path_is_within};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio_util::sync::CancellationToken;

pub(crate) struct SyncState(pub(crate) AtomicBool);

pub(crate) struct SyncCoordinator(pub(crate) std::sync::Arc<SyncCoordinatorInner>);

pub(crate) struct SyncCoordinatorInner {
    pub(crate) lock: tokio::sync::Mutex<()>,
    pub(crate) paused: AtomicBool,
    pub(crate) resume: tokio::sync::Notify,
    active_work: tokio::sync::Mutex<Vec<ActiveWork>>,
    next_work_id: AtomicU64,
}

struct ActiveWork {
    id: u64,
    path: PathBuf,
    cancellation: CancellationToken,
}

pub(crate) struct ActiveWorkRegistration {
    id: u64,
}

impl SyncCoordinatorInner {
    pub(crate) fn new(paused: bool) -> Self {
        Self {
            lock: tokio::sync::Mutex::new(()),
            paused: AtomicBool::new(paused),
            resume: tokio::sync::Notify::new(),
            active_work: tokio::sync::Mutex::new(Vec::new()),
            next_work_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn register_active_work(
        &self,
        paths: impl IntoIterator<Item = String>,
    ) -> Vec<ActiveWorkRegistration> {
        let mut active_work = self.active_work.lock().await;
        paths
            .into_iter()
            .map(|path| {
                let id = self.next_work_id.fetch_add(1, Ordering::Relaxed);
                let cancellation = CancellationToken::new();
                active_work.push(ActiveWork {
                    id,
                    path: normalize_folder_path(&PathBuf::from(path)),
                    cancellation: cancellation.clone(),
                });
                ActiveWorkRegistration { id }
            })
            .collect()
    }

    pub(crate) async fn unregister_active_work(&self, registrations: &[ActiveWorkRegistration]) {
        let ids: Vec<_> = registrations
            .iter()
            .map(|registration| registration.id)
            .collect();
        self.active_work
            .lock()
            .await
            .retain(|work| !ids.contains(&work.id));
    }

    pub(crate) async fn cancellation_for_path(&self, path: &str) -> Option<CancellationToken> {
        let path = normalize_folder_path(&PathBuf::from(path));
        self.active_work.lock().await.iter().find_map(|work| {
            (path_is_within(&work.path, &path) && path_is_within(&path, &work.path))
                .then(|| work.cancellation.clone())
        })
    }

    pub(crate) async fn has_active_work_in(&self, folder: &str) -> bool {
        let folder = normalize_folder_path(&PathBuf::from(folder));
        self.active_work
            .lock()
            .await
            .iter()
            .any(|work| path_is_within(&work.path, &folder))
    }

    pub(crate) async fn cancel_work_in(&self, folder: &str) {
        let folder = normalize_folder_path(&PathBuf::from(folder));
        for work in self.active_work.lock().await.iter() {
            if path_is_within(&work.path, &folder) {
                work.cancellation.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SyncCoordinatorInner;

    #[tokio::test]
    async fn cancellation_lookup_is_case_insensitive_for_windows_paths() {
        let coordinator = SyncCoordinatorInner::new(false);
        coordinator
            .register_active_work([r"C:\Photos\active.jpg".to_string()])
            .await;

        let cancellation = coordinator
            .cancellation_for_path(r"c:\photos\ACTIVE.jpg")
            .await
            .expect("the same Windows path must find its cancellation token");
        coordinator.cancel_work_in(r"c:\PHOTOS").await;

        assert!(cancellation.is_cancelled());
    }
}
