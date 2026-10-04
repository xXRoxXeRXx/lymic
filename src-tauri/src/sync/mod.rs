//! Crate-private synchronization facade.
//!
//! Commands and application tasks enter synchronization through this module so
//! the Tauri surface remains separate from queue coordination and processing.

pub(crate) mod coordinator;
pub(crate) mod processor;
pub(crate) mod queue_runner;
pub(crate) mod retry;
pub(crate) mod service;

pub(crate) use coordinator::{SyncCoordinator, SyncCoordinatorInner, SyncState};
pub(crate) use processor::{load_upload_parallelism, SyncSummary};
pub(crate) use queue_runner::run_sync_pipeline;
pub(crate) use retry::prepare_failed_sync_retry;
pub(crate) use service::{
    add_event_paths, create_authenticated_client, ensure_sync_has_folders, log_overlapping_folders,
    scan_folders_for_media, sync_scan_result_if_authenticated, Asset, FileFailure, ScanResult,
};
