//! Reconciliation as the application runs it (#280): the populated-directory record in SQLite, the
//! one guard shared by the schedule and the Refresh Library command, and the schedule itself.
//!
//! The decisions are in `diff`, `reconcile` and `refresh`; this only wires them to the real Immich
//! session, the database and (on Windows) the disk.

use super::reconcile::{PopulatedDirectories, Report, RunGate};
use super::refresh::{self, SingleFlight, TokioClock, Trigger};
use crate::db;
use crate::virtual_library_root::{is_available_at_runtime, load_root, SystemKnownFolders};
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::{Mutex, OnceLock};

/// The database the Cloud Files callbacks record populated directories in. Set when the schedule
/// starts at application start, before the sync root is served.
static DATABASE: OnceLock<SqlitePool> = OnceLock::new();
/// The one guard every run goes through, and the one cleanup waits on.
static FLIGHT: OnceLock<SingleFlight<Report>> = OnceLock::new();
/// Directories the platform already has but whose record failed; retried at the start of each run.
static UNRECORDED: Unrecorded = Unrecorded::new();

fn flight() -> &'static SingleFlight<Report> {
    FLIGHT.get_or_init(SingleFlight::new)
}

/// Runs `work` while no reconciliation run is in progress and none can start: it waits for a run
/// that is going. Disabling and logout run their whole cleanup inside it, so a run can neither
/// create placeholders after the cleanup scanned nor delete under it.
pub(crate) async fn while_reconciliation_is_paused<R>(work: impl Future<Output = R>) -> R {
    flight().exclusive(|| work).await
}

/// Forgets the directories whose record failed and was never retried. Cleanup calls it where it
/// clears the record of populated directories: their placeholders are gone, so a retry after the
/// Virtual Library is enabled again must not mark them populated.
pub(crate) fn forget_unrecorded() {
    UNRECORDED.forget_all();
}

/// Where a populated directory is recorded.
trait PopulatedStore: Sync {
    fn record(&self, path: &str) -> impl Future<Output = Result<(), String>> + Send;
}

/// The record in SQLite, `None` while no database was installed.
struct Recording(Option<&'static SqlitePool>);

impl PopulatedStore for Recording {
    async fn record(&self, path: &str) -> Result<(), String> {
        let Some(pool) = self.0 else {
            return Err("no database to record it in".to_string());
        };
        db::mark_virtual_library_directory_populated(pool, path)
            .await
            .map_err(|error| error.to_string())
    }
}

/// The populated directories whose record failed. A directory the platform has been given but that
/// is not recorded is never reconciled and never listed again, so a failure is kept here and
/// retried by the next run instead of being forgotten.
struct Unrecorded {
    paths: Mutex<BTreeSet<String>>,
}

impl Unrecorded {
    const fn new() -> Self {
        Self {
            paths: Mutex::new(BTreeSet::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    fn pending(&self) -> Vec<String> {
        self.lock().iter().cloned().collect()
    }

    fn forget_all(&self) {
        self.lock().clear();
    }

    /// Records `path`; when that fails it stays pending and the error is returned.
    async fn mark(&self, store: &impl PopulatedStore, path: &str) -> Result<(), String> {
        match store.record(path).await {
            Ok(()) => {
                self.lock().remove(path);
                Ok(())
            }
            Err(error) => {
                self.lock().insert(path.to_string());
                Err(error)
            }
        }
    }

    /// Tries again for every pending directory; the first failure stops and keeps the rest.
    async fn retry(&self, store: &impl PopulatedStore) -> Result<(), String> {
        let pending: Vec<String> = self.lock().iter().cloned().collect();
        for path in pending {
            store.record(&path).await?;
            self.lock().remove(&path);
        }
        Ok(())
    }
}

/// Whether the Virtual Library is still on, read from the database at the moment of asking. A
/// database that cannot say counts as off: nothing is changed on doubt.
struct StillEnabled<'a>(&'a SqlitePool);

impl RunGate for StillEnabled<'_> {
    async fn still_enabled(&self) -> bool {
        is_available_at_runtime()
            && matches!(db::get_virtual_library_enabled(self.0).await, Ok(true))
    }
}

/// The directories recorded in SQLite.
struct SqlitePopulated<'a>(&'a SqlitePool);

impl PopulatedDirectories for SqlitePopulated<'_> {
    async fn all(&self) -> Result<Vec<String>, String> {
        db::list_virtual_library_populated_directories(self.0)
            .await
            .map_err(|error| error.to_string())
    }

    async fn forget_tree(&self, path: &str) -> Result<(), String> {
        db::forget_virtual_library_populated_tree(self.0, path)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Gives the callbacks their database before the sync root is served, so that no populated
/// directory is lost to the time between the connection and the start of the schedule.
pub(crate) fn use_database(pool: &SqlitePool) {
    let _ = DATABASE.set(pool.clone());
}

/// Records that the directory at the root-relative `path` was populated, so reconciliation diffs it
/// from now on. Called by the `FETCH_PLACEHOLDERS` callback once the platform has the placeholders.
/// A failure is returned for the caller to make loud, and the directory is retried by the next
/// reconciliation run: the platform will not ask for that listing again.
pub(crate) async fn mark_populated(path: &str) -> Result<(), String> {
    UNRECORDED.mark(&Recording(DATABASE.get()), path).await
}

/// One run: nothing when the Virtual Library is unavailable or off.
async fn run_once(pool: &SqlitePool) -> Report {
    let mut report = Report::default();
    if !is_available_at_runtime() {
        return report;
    }
    match db::get_virtual_library_enabled(pool).await {
        Ok(true) => {}
        Ok(false) => return report,
        Err(error) => {
            report.failure = Some(error.to_string());
            return report;
        }
    }
    let root = match load_root(pool, &SystemKnownFolders).await {
        Ok(Some(root)) => root,
        Ok(None) => return report,
        Err(error) => {
            report.failure = Some(error);
            return report;
        }
    };
    let populated = SqlitePopulated(pool);
    // Directories the platform has but whose record failed earlier: recorded now, or reported.
    let unrecorded_failure = UNRECORDED
        .retry(&Recording(DATABASE.get()))
        .await
        .err()
        .map(|error| format!("could not record a populated directory: {error}"));
    #[cfg(windows)]
    {
        let directories = match populated.all().await {
            Ok(directories) => directories,
            Err(error) => {
                report.failure = Some(error);
                return report;
            }
        };
        let tree = super::adapter::disk_tree(root, &directories);
        let immich = super::library::StoredCredentialsImmich;
        // Lists through the thumbnail cache filler, so new assets get their thumbnail too (#282).
        let library = super::shell_thumbnails::WarmingLibrary::system(immich);
        let mut run =
            super::reconcile::reconcile(&library, &immich, &tree, &populated, &StillEnabled(pool))
                .await;
        if run.failure.is_none() {
            run.failure = unrecorded_failure;
        }
        run
    }
    #[cfg(not(windows))]
    {
        let _ = (root, populated, unrecorded_failure);
        report.failure = Some("the Virtual Library needs Windows".to_string());
        report
    }
}

fn log(trigger: Trigger, report: &Report) {
    if let Some(failure) = &report.failure {
        super::system_note(&format!("reconciliation ({trigger:?}) failed: {failure}"));
    }
}

/// Runs reconciliation at application start and then every ten minutes, for the life of the
/// process. A run that finds one in progress does nothing (see `refresh`).
pub(crate) async fn run_schedule(pool: SqlitePool) {
    let _ = DATABASE.set(pool.clone());
    refresh::run_schedule(&TokioClock, refresh::INTERVAL, |trigger| {
        let pool = pool.clone();
        async move {
            if let Some(report) = flight().run_unless_busy(|| run_once(&pool)).await {
                log(trigger, &report);
            }
        }
    })
    .await;
}

/// The Refresh Library action: waits for a run in progress and then runs once more, or joins the
/// follow-up run another refresh already waits for (see `refresh`).
pub(crate) async fn refresh_now(pool: &SqlitePool) -> Report {
    let report = flight().run_after_current(|| run_once(pool)).await;
    log(Trigger::Manual, &report);
    report
}

/// What `commands/virtual_library.rs` calls to refresh, behind a trait so the command is tested
/// without Windows.
pub(crate) trait LibraryRefresh: Sync {
    /// Reconciles now; the error is the first thing that went wrong, if anything did.
    fn refresh(&self) -> impl Future<Output = Result<(), String>> + Send;
}

/// The real thing.
pub(crate) struct SystemRefresh {
    pub(crate) pool: SqlitePool,
}

impl LibraryRefresh for SystemRefresh {
    async fn refresh(&self) -> Result<(), String> {
        match refresh_now(&self.pool).await.failure {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A record that fails while told to and remembers what it was given.
    struct FakeStore {
        failing: AtomicBool,
        recorded: Mutex<Vec<String>>,
    }

    impl FakeStore {
        fn new(failing: bool) -> Self {
            Self {
                failing: AtomicBool::new(failing),
                recorded: Mutex::new(Vec::new()),
            }
        }
    }

    impl PopulatedStore for FakeStore {
        async fn record(&self, path: &str) -> Result<(), String> {
            if self.failing.load(Ordering::SeqCst) {
                return Err("database is locked".to_string());
            }
            self.recorded.lock().unwrap().push(path.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_directory_whose_record_failed_is_reported_and_recorded_by_the_next_run() {
        let unrecorded = Unrecorded::new();
        let store = FakeStore::new(true);

        let result = unrecorded.mark(&store, "Albums/Trip").await;

        assert_eq!(result, Err("database is locked".to_string()));
        assert!(store.recorded.lock().unwrap().is_empty());

        store.failing.store(false, Ordering::SeqCst);
        unrecorded.retry(&store).await.unwrap();

        assert_eq!(*store.recorded.lock().unwrap(), ["Albums/Trip"]);
        // Recorded once: a later run has nothing left to retry.
        unrecorded.retry(&store).await.unwrap();
        assert_eq!(store.recorded.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_retry_that_fails_again_keeps_the_directory_pending() {
        let unrecorded = Unrecorded::new();
        let store = FakeStore::new(true);
        let _ = unrecorded.mark(&store, "Albums").await;

        assert!(unrecorded.retry(&store).await.is_err());

        store.failing.store(false, Ordering::SeqCst);
        unrecorded.retry(&store).await.unwrap();
        assert_eq!(*store.recorded.lock().unwrap(), ["Albums"]);
    }

    #[tokio::test]
    async fn a_directory_recorded_at_once_is_never_pending() {
        let unrecorded = Unrecorded::new();
        let store = FakeStore::new(false);

        unrecorded.mark(&store, "All Photos").await.unwrap();
        unrecorded.retry(&FakeStore::new(true)).await.unwrap();

        assert_eq!(*store.recorded.lock().unwrap(), ["All Photos"]);
    }

    #[tokio::test]
    async fn without_an_installed_database_the_record_fails_instead_of_being_dropped() {
        let unrecorded = Unrecorded::new();

        let result = unrecorded.mark(&Recording(None), "Albums").await;

        assert!(result.is_err());
        assert_eq!(unrecorded.pending(), ["Albums"], "kept for the next run");
    }

    #[tokio::test]
    async fn pending_records_are_forgotten_once_the_directories_are_cleaned_up() {
        let unrecorded = Unrecorded::new();
        let store = FakeStore::new(true);
        let _ = unrecorded.mark(&store, "Albums").await;

        unrecorded.forget_all();
        store.failing.store(false, Ordering::SeqCst);
        unrecorded.retry(&store).await.unwrap();

        assert!(
            store.recorded.lock().unwrap().is_empty(),
            "the placeholders are gone, so the old directory must not come back as populated"
        );
    }
}
