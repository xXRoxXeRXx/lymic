mod exclusions;
mod registration;
mod removal;
mod seams;

use seams::{
    CoordinatorSyncWork, FolderEffects, FolderPersistence, FolderSyncWork, SqliteFolderPersistence,
    TauriFolderEffects,
};

pub(crate) use registration::RegisterFolderParams;
#[cfg(test)]
pub(crate) use registration::RemovableVolumeBinding;
pub(crate) use removal::RemoveFolderResult;
pub(crate) use seams::{
    FolderScanTrigger, FolderWatcher, NotifyFolderWatcher, TauriFolderScanTrigger,
};

pub(crate) struct WatchedFolders<P, W, S, E> {
    persistence: P,
    watcher: W,
    sync: S,
    effects: E,
}

impl<'a>
    WatchedFolders<
        SqliteFolderPersistence<'a>,
        NotifyFolderWatcher<'a>,
        CoordinatorSyncWork<'a>,
        TauriFolderEffects<'a>,
    >
{
    pub(crate) fn for_tauri(app: &'a tauri::AppHandle, pool: &'a sqlx::SqlitePool) -> Self {
        use tauri::Manager;
        let sync_coordinator = app.state::<crate::sync::SyncCoordinator>();
        let running = app.state::<crate::sync::SyncState>().0.clone();
        Self::new(
            SqliteFolderPersistence { pool },
            NotifyFolderWatcher::for_tauri(app),
            CoordinatorSyncWork {
                pool,
                running,
                coordinator: sync_coordinator.0.clone(),
            },
            TauriFolderEffects { app, pool },
        )
    }
}

impl<P, W, S, E> WatchedFolders<P, W, S, E>
where
    P: FolderPersistence,
    W: FolderWatcher,
    S: FolderSyncWork,
    E: FolderEffects,
{
    pub(crate) fn new(persistence: P, watcher: W, sync: S, effects: E) -> Self {
        Self {
            persistence,
            watcher,
            sync,
            effects,
        }
    }

    pub(crate) async fn register(&self, input: RegisterFolderParams) -> Result<i64, String> {
        registration::register_folder(&self.persistence, &self.watcher, &self.effects, input).await
    }

    pub(crate) async fn update_exclusion_patterns(
        &self,
        id: i64,
        exclusion_patterns: Vec<String>,
    ) -> Result<(), String> {
        exclusions::update_folder_exclusion_patterns(
            &self.persistence,
            &self.sync,
            &self.effects,
            id,
            exclusion_patterns,
        )
        .await
    }

    pub(crate) async fn remove(
        &self,
        id: i64,
        confirm_cancellation: Option<bool>,
    ) -> Result<RemoveFolderResult, String> {
        removal::remove_folder(
            &self.persistence,
            &self.watcher,
            &self.sync,
            &self.effects,
            id,
            confirm_cancellation,
        )
        .await
    }
}

#[cfg(test)]
mod tests;
