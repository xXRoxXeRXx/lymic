use crate::sync::ActiveWork;
use std::sync::{atomic::AtomicBool, Arc};

pub(crate) struct SyncState(pub(crate) std::sync::Arc<AtomicBool>);

pub(crate) struct SyncCoordinator(pub(crate) std::sync::Arc<SyncCoordinatorInner>);

pub(crate) struct SyncCoordinatorInner {
    pub(crate) lock: tokio::sync::Mutex<()>,
    pub(crate) paused: AtomicBool,
    pub(crate) resume: tokio::sync::Notify,
    pub(crate) active_work: Arc<ActiveWork>,
}

impl SyncCoordinatorInner {
    pub(crate) fn new(paused: bool) -> Self {
        Self {
            lock: tokio::sync::Mutex::new(()),
            paused: AtomicBool::new(paused),
            resume: tokio::sync::Notify::new(),
            active_work: Arc::new(ActiveWork::new()),
        }
    }
}
