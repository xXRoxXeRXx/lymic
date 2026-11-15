use super::seams::{FolderEffects, FolderPersistence, FolderSyncWork, FolderWatcher};
use crate::audit::{AuditEvent, Outcome, Severity};
use serde_json::json;
use std::path::PathBuf;

fn removal_requires_confirmation(confirm_cancellation: Option<bool>) -> bool {
    !confirm_cancellation.unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveFolderResult {
    pub(crate) requires_sync_cancellation: bool,
}

pub(crate) async fn remove_folder<P, W, S, E>(
    persistence: &P,
    watcher: &W,
    sync: &S,
    effects: &E,
    id: i64,
    confirm_cancellation: Option<bool>,
) -> Result<RemoveFolderResult, String>
where
    P: FolderPersistence,
    W: FolderWatcher,
    S: FolderSyncWork,
    E: FolderEffects,
{
    let folders = persistence.list_folders().await?;
    let folder = folders.into_iter().find(|folder| folder.id == id);

    // Unconfirmed calls never mutate state, refresh tray, or emit audit events—even if the ID is absent.
    if removal_requires_confirmation(confirm_cancellation) {
        let requires_sync_cancellation = match folder {
            Some(ref folder) => sync.has_work_in_folder(&folder.path).await?,
            None => false,
        };
        return Ok(RemoveFolderResult {
            requires_sync_cancellation,
        });
    }

    if let Some(folder) = folder {
        if let Err(error) = watcher.unwatch(&folder.path).await {
            effects.log_unwatch_failed(&folder.path, &error);
            return Err(format!("Failed to unwatch '{}': {}", folder.path, error));
        }

        // The watcher is gone before cancellation so no new work can be produced for this root.
        // The queue cleanup is path-component based and leaves sibling roots untouched.
        sync.cancel_work_in_folder(&folder.path);
        let discarded_and_deleted = match sync.discard_folder_queue(&folder.path).await {
            Ok(()) => persistence.remove_folder(id).await,
            Err(error) => Err(error),
        };
        if let Err(error) = discarded_and_deleted {
            restore_watcher_after_failure(watcher, effects, &folder.path).await;
            return Err(error);
        }
    } else {
        persistence.remove_folder(id).await?;
    }

    match persistence.list_folders().await {
        Ok(folders) => effects.set_sync_enabled(!folders.is_empty()),
        Err(error) => effects.log_tray_refresh_after_remove_failed(&error),
    }

    effects.record_audit(AuditEvent::new(
        AuditEvent::operation_id(),
        "folder.removed",
        Outcome::Success,
        Severity::Info,
        None,
        "command",
        "Watched folder removed",
        json!({ "folder_id": id }),
    ));

    Ok(RemoveFolderResult {
        requires_sync_cancellation: false,
    })
}

async fn restore_watcher_after_failure<W: FolderWatcher, E: FolderEffects>(
    watcher: &W,
    effects: &E,
    path: &str,
) {
    let path_for_check = PathBuf::from(path);
    let is_available = tokio::task::spawn_blocking(move || path_for_check.is_dir())
        .await
        .unwrap_or(false);
    if is_available {
        if let Err(rewatch_error) = watcher.watch_available(path).await {
            effects.log_watcher_restore_failed(path, &rewatch_error);
        }
    }
}
