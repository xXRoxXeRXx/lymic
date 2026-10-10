#![allow(unused_imports)]

mod folders;
mod init;
mod queue;
mod settings;
mod sync_state;

pub use folders::{add_folder, get_folders, remove_folder, restore_folder, WatchedFolder};
pub use init::{init, init_at_path, DatabaseStatus, DbInitResult};
pub use queue::{
    complete_sync_job_if_finished, enqueue_sync_assets, finalize_queued_block, get_sync_snapshot,
    has_pending_sync_queue_items, next_sync_queue_block, recover_sync_job, set_sync_status,
    QueueAsset, QueuedAsset, SyncSnapshot,
};
pub use settings::{
    get_upload_parallelism, set_upload_parallelism, validate_upload_parallelism,
    DEFAULT_UPLOAD_PARALLELISM,
};
pub use sync_state::{
    discard_failed_sync_paths, discard_sync_data_in_folder, get_cached_hash, get_failed_syncs,
    has_sync_work_in_folder, mark_sync_failed, update_sync_state, FailedSyncEntry,
};

#[cfg(test)]
mod tests;
