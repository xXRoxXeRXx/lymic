//! Crate-private synchronization facade.
//!
//! Commands and application tasks enter synchronization through this module so
//! the Tauri surface remains separate from queue coordination and processing.

pub(crate) mod active_work;
pub(crate) mod album_batch;
pub(crate) mod completion;
pub(crate) mod coordinator;
pub(crate) mod processor;
pub(crate) mod progress;
pub(crate) mod queue_runner;
pub(crate) mod retry;
pub(crate) mod service;

pub(crate) use active_work::{ActiveWork, ProcessingContext};
pub(crate) use coordinator::{SyncCoordinator, SyncCoordinatorInner, SyncState};
pub(crate) use processor::{load_upload_parallelism, SyncSummary};
pub(crate) use progress::JobByteProgress;
pub(crate) use retry::prepare_failed_sync_retry;
pub(crate) use service::{
    add_event_paths, admit_and_run, log_overlapping_folders, scan_folders_for_media,
    scan_watched_folders_for_media, Asset, FileFailure, ScanResult, SyncSource, Synchronization,
};
