mod folders;
mod init;
mod queue;
mod settings;
mod sync_state;

pub use folders::{
    add_folder, bind_removable_folder, get_folders, remap_watched_folder_path, remove_folder,
    WatchedFolder,
};
#[allow(unused_imports)]
pub use folders::{
    get_removable_folder_binding, get_removable_folders_by_volume, unbind_removable_folder,
    RemovableFolderBinding,
};
pub use init::{init, DatabaseStatus};
#[allow(unused_imports)]
pub use queue::SINGLETON_SYNC_JOB_ID;
pub use queue::{
    complete_sync_job_if_finished, enqueue_upload_units, finalize_queued_block, get_sync_snapshot,
    has_pending_sync_queue_items, next_queued_upload_units, recover_sync_job,
    reset_failed_queue_items_for_folder, set_sync_status, update_sync_queue_failure,
    update_sync_queue_remote_id, QueueUploadAsset, QueueUploadUnit, QueuedUploadUnit, SyncSnapshot,
};

/// Builds a SQL WHERE predicate condition matching a path column against a root prefix or any of its descendants.
///
/// Prevents false prefix matches against sibling folders (e.g. `/media/drive2` matching `/media/drive`).
pub(crate) fn sql_path_is_or_descendant_of(column: &str, param_placeholder: &str) -> String {
    format!(
        "({col} = {param} \
         OR substr({col}, 1, length({param}) + 1) = {param} || '/' \
         OR substr({col}, 1, length({param}) + 1) = {param} || '\\\\')",
        col = column,
        param = param_placeholder
    )
}

#[cfg(test)]
pub use queue::{
    enqueue_sync_assets, next_sync_queue_block, update_upload_unit_remote_id, QueueAsset,
};
pub use settings::{
    get_upload_parallelism, get_virtual_library_status, set_upload_parallelism,
    set_virtual_library_enabled, VirtualLibraryStatus,
};
pub use sync_state::{
    discard_failed_sync_paths, discard_sync_data_in_folder, get_cached_hash, get_failed_syncs,
    has_sync_work_in_folder, mark_sync_failed, persist_uploaded_unit, update_sync_state,
    FailedSyncEntry, UploadedAssetState,
};

#[cfg(test)]
pub use init::init_at_path;
#[cfg(test)]
pub use settings::{
    validate_upload_parallelism, DEFAULT_UPLOAD_PARALLELISM, DEFAULT_VIRTUAL_LIBRARY_PATH,
};

#[cfg(test)]
mod tests;
