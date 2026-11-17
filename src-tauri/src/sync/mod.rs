//! Crate-private synchronization facade.
//!
//! Commands and application tasks enter synchronization through this module so
//! the Tauri surface remains separate from queue coordination and processing.

pub(crate) mod active_work;
pub(crate) mod admission;
pub(crate) mod album_batch;
pub(crate) mod completion;
pub(crate) mod lifecycle;
pub(crate) mod processor;
pub(crate) mod progress;
pub(crate) mod queue_runner;
pub(crate) mod reporter;
pub(crate) mod retry;
pub(crate) mod runtime;
pub(crate) mod service;

#[cfg(test)]
pub(crate) use active_work::ActiveWork;
pub(crate) use active_work::ProcessingContext;
pub(crate) use processor::{load_upload_parallelism, SyncSummary};
pub(crate) use progress::{CurrentFileProgress, JobByteProgress};
pub(crate) use runtime::SyncRuntime;
pub(crate) use service::{SyncSource, Synchronization};
