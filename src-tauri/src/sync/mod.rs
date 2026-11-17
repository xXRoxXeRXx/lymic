//! Crate-private synchronization facade.
//!
//! Commands and application tasks enter synchronization through this module so
//! the Tauri surface remains separate from queue coordination and processing.

pub(crate) mod active_work;
pub(crate) mod album_batch;
pub(crate) mod completion;
pub(crate) mod coordinator;
pub(crate) mod lifecycle;
pub(crate) mod processor;
pub(crate) mod progress;
pub(crate) mod queue_runner;
pub(crate) mod reporter;
pub(crate) mod retry;
pub(crate) mod service;

pub(crate) use active_work::{ActiveWork, ProcessingContext};
pub(crate) use coordinator::{SyncCoordinator, SyncCoordinatorInner, SyncState};
pub(crate) use processor::{load_upload_parallelism, SyncSummary};
pub(crate) use progress::{CurrentFileProgress, JobByteProgress};
pub(crate) use retry::prepare_failed_sync_retry;
pub(crate) use service::{admit_and_run, SyncSource, Synchronization};
