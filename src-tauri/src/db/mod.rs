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

/// Normalizes a folder path into the prefix format expected by `sql_path_is_or_descendant_of`.
pub(crate) fn sql_folder_prefix(folder: &str) -> String {
    let norm = crate::media::paths::normalize_path_lexical(std::path::Path::new(folder));
    let raw = norm.to_string_lossy();
    #[cfg(windows)]
    {
        let slashed = raw.replace('\\', "/");
        if slashed == "/" || (slashed.len() == 3 && slashed.ends_with(":/")) {
            slashed
        } else {
            slashed.trim_end_matches('/').to_owned()
        }
    }
    #[cfg(not(windows))]
    {
        if raw == "/" {
            "/".to_owned()
        } else {
            raw.trim_end_matches('/').to_owned()
        }
    }
}

/// Builds a SQL WHERE predicate pre-filtering paths matching `param_placeholder`
/// (prepared with `sql_folder_prefix`) or any of its descendants.
///
/// Rejects sibling folders and unrelated Watched Folders directly in SQL while
/// passing through relative dot-segment or Windows non-ASCII rows for exact Rust
/// verification via `media::paths`.
pub(crate) fn sql_path_is_or_descendant_of(column: &str, param_placeholder: &str) -> String {
    #[cfg(windows)]
    {
        // Strip Windows verbatim prefixes (`\\?\UNC\` -> `\\`, `\\?\X:` -> `X:`) and unify slashes.
        let unverbatim = format!(
            "(CASE \
             WHEN substr({col}, 1, 8) = '\\\\?\\UNC\\' THEN '//' || substr({col}, 9) \
             WHEN substr({col}, 1, 4) = '\\\\?\\' AND substr({col}, 6, 1) = ':' THEN substr({col}, 5) \
             ELSE {col} END)",
            col = column
        );
        let slash_col = format!("replace({unverbatim}, '\\', '/')");
        let trimmed_col = format!(
            "(CASE WHEN {s} = '/' OR (length({s}) = 3 AND substr({s}, 2, 2) = ':/') THEN {s} ELSE rtrim({s}, '/') END)",
            s = slash_col
        );
        format!(
            "({param} != '' AND (\
             {norm} = {param} COLLATE NOCASE \
             OR (substr({param}, length({param}), 1) = '/' AND substr({norm}, 1, length({param})) = {param} COLLATE NOCASE) \
             OR (substr({param}, length({param}), 1) != '/' AND substr({norm}, 1, length({param}) + 1) = ({param} || '/') COLLATE NOCASE) \
             OR {col} LIKE '%..%' OR {col} LIKE '%/.%' OR {col} LIKE '%\\.%' \
             OR substr({s}, 1, 3) = '///' \
             OR (substr({s}, 1, 2) = '//' AND instr(substr({s}, 3), '//') > 0) \
             OR (substr({s}, 1, 2) != '//' AND instr({s}, '//') > 0) \
             OR {col} GLOB '*[^ -~]*' OR {param} GLOB '*[^ -~]*'))",
            col = column,
            s = slash_col,
            norm = trimmed_col,
            param = param_placeholder
        )
    }

    #[cfg(not(windows))]
    {
        let trimmed_col = format!(
            "(CASE WHEN {col} = '/' THEN '/' ELSE rtrim({col}, '/') END)",
            col = column
        );
        format!(
            "({param} != '' AND (\
             {norm} = {param} \
             OR ({param} = '/' AND substr({norm}, 1, 1) = '/') \
             OR ({param} != '/' AND substr({norm}, 1, length({param}) + 1) = {param} || '/') \
             OR {col} LIKE '%..%' OR {col} LIKE '%/.%' OR {col} LIKE '%//%'))",
            col = column,
            norm = trimmed_col,
            param = param_placeholder
        )
    }
}

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
    let folder = std::path::Path::new(folder);
    paths
        .into_iter()
        .filter(|path| crate::media::paths::path_is_within(std::path::Path::new(path), folder))
        .collect()
}

#[cfg(test)]
pub(crate) async fn test_pool() -> sqlx::SqlitePool {
    let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite:file:{database_name}?mode=memory&cache=private"
        ))
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
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
