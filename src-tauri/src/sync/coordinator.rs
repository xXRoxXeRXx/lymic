use std::sync::atomic::AtomicBool;

pub(crate) struct SyncState(pub(crate) AtomicBool);

pub(crate) struct SyncCoordinator(pub(crate) std::sync::Arc<SyncCoordinatorInner>);

pub(crate) struct SyncCoordinatorInner {
    pub(crate) lock: tokio::sync::Mutex<()>,
    pub(crate) paused: AtomicBool,
    pub(crate) resume: tokio::sync::Notify,
}
