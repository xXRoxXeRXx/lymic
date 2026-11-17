mod folders;
mod init;
mod queue;
mod settings;
mod sync_state;

pub use folders::{
    add_folder, bind_removable_folder, get_folders, remap_watched_folder_path, remove_folder,
    update_exclusion_patterns, WatchedFolder,
};
pub use folders::{get_removable_folders_by_volume, RemovableFolderBinding};
pub use init::{init, DatabaseStatus};
pub use queue::{
    complete_sync_job_if_finished, discard_pending_sync_paths, enqueue_upload_units,
    finalize_queued_block, get_sync_snapshot, has_pending_sync_queue_items,
    next_queued_upload_units, reset_failed_queue_items_for_folder, set_sync_status,
    update_sync_queue_failure, update_sync_queue_remote_id, QueueUploadAsset, QueueUploadUnit,
    QueuedUploadUnit, SyncJobStatus, SyncSnapshot,
};

pub(super) const PATH_BATCH_SIZE: usize = 500;

pub(super) async fn execute_batched_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    paths: &[String],
    mut build_prefix: impl FnMut(&mut sqlx::QueryBuilder<'_, sqlx::Sqlite>),
) -> Result<u64, sqlx::Error> {
    let mut total_affected = 0;
    for path_batch in paths.chunks(PATH_BATCH_SIZE) {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
        build_prefix(&mut query);
        query.push(" IN (");
        let mut separated = query.separated(", ");
        for path in path_batch {
            separated.push_bind(path);
        }
        separated.push_unseparated(")");
        total_affected += query.build().execute(&mut **tx).await?.rows_affected();
    }
    Ok(total_affected)
}

pub(crate) fn filter_paths_within_folder(paths: Vec<String>, folder: &str) -> Vec<String> {
    let norm_folder = crate::media::paths::normalize_path_lexical(std::path::Path::new(folder));
    paths
        .into_iter()
        .filter(|path| {
            crate::media::paths::path_is_within_lexical(
                &crate::media::paths::normalize_path_lexical(std::path::Path::new(path)),
                &norm_folder,
            )
        })
        .collect()
}

#[cfg(test)]
pub use queue::{
    enqueue_sync_assets, next_sync_queue_block, update_upload_unit_remote_id, QueueAsset,
};
pub use settings::{get_upload_parallelism, set_upload_parallelism};
pub use sync_state::{
    discard_failed_sync_paths, discard_sync_data_in_folder, get_cached_hash, get_failed_syncs,
    has_sync_work_in_folder, mark_sync_failed, persist_uploaded_unit, update_sync_state,
    FailedSyncEntry, UploadedAssetState,
};

#[cfg(test)]
pub use folders::{get_removable_folder_binding, unbind_removable_folder};
#[cfg(test)]
pub use init::init_at_path;
#[cfg(test)]
pub use settings::{validate_upload_parallelism, DEFAULT_UPLOAD_PARALLELISM};

#[cfg(test)]
mod tests;
