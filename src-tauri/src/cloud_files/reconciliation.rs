//! Reconciliation as the application runs it (#280): the populated-directory record in SQLite, the
//! one guard shared by the schedule and the Refresh Library command, and the schedule itself.
//!
//! The decisions are in `diff`, `reconcile` and `refresh`; this only wires them to the real Immich
//! session, the database and (on Windows) the disk.

use super::reconcile::{PopulatedDirectories, Report};
use super::refresh::{self, SingleFlight, TokioClock, Trigger};
use crate::db;
use crate::virtual_library_root::{is_available_at_runtime, load_root, SystemKnownFolders};
use sqlx::SqlitePool;
use std::future::Future;
use std::sync::OnceLock;

/// The database the Cloud Files callbacks record populated directories in. Set when the schedule
/// starts at application start, before the sync root is served.
static DATABASE: OnceLock<SqlitePool> = OnceLock::new();
/// The one guard every run goes through.
static FLIGHT: OnceLock<SingleFlight<Report>> = OnceLock::new();

fn flight() -> &'static SingleFlight<Report> {
    FLIGHT.get_or_init(SingleFlight::new)
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

/// Records that the directory at the root-relative `path` was populated, so reconciliation diffs it
/// from now on. Called by the `FETCH_PLACEHOLDERS` callback once the platform has the placeholders.
pub(crate) async fn mark_populated(path: &str) {
    let Some(pool) = DATABASE.get() else {
        eprintln!("virtual library: no database to record the populated directory '{path}'");
        return;
    };
    if let Err(error) = db::mark_virtual_library_directory_populated(pool, path).await {
        eprintln!("virtual library: could not record the populated directory '{path}': {error}");
    }
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
        super::reconcile::reconcile(&immich, &immich, &tree, &populated).await
    }
    #[cfg(not(windows))]
    {
        let _ = (root, populated);
        report.failure = Some("the Virtual Library needs Windows".to_string());
        report
    }
}

fn log(trigger: Trigger, report: &Report) {
    if let Some(failure) = &report.failure {
        eprintln!("virtual library: reconciliation ({trigger:?}) failed: {failure}");
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
