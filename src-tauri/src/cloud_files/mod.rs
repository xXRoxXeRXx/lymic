//! The Windows adapter of the read-only Virtual Library (ADR-0003, #278).
//!
//! It registers the sync root, creates placeholders for a directory when Explorer first lists it and
//! hydrates the full original when a placeholder is opened. It is deliberately thin: every rule
//! about names, paths, dates, Live Photo pairs and collisions lives in `virtual_library`, and
//! this module only translates Cloud Files callbacks into `Library::list` and `Library::resolve`.
//!
//! Everything except the Windows calls compiles and is tested on every platform. The calls
//! themselves are in `adapter`, behind `cfg(windows)`; they cannot be unit tested and are covered
//! by the manual record in `docs/virtual-library-verification.md`.
//!
//! Lifecycle: [`SystemSyncRoot::start`] runs when the user enables the Virtual Library and again
//! on app start while it is enabled. [`SystemSyncRoot::clean_up`] (disabling, logout; `cleanup`,
//! #283) stops serving, unregisters the sync root and removes what Lymic created.
//!
//! Reconciliation (#280) keeps populated directories current: `reconcile` diffs what
//! `population::populate` plans (`diff`) against the directories on disk, `refresh` decides when it
//! runs and keeps runs from overlapping, `reconciliation` wires it to the app.
//!
//! Hooks for the follow-up tickets: #281 (done) is `failure`, `reporting` and `inflight`.

// Outside Windows only the tests use most of this module.
#![cfg_attr(not(windows), allow(dead_code))]

mod activation;
mod cleanup;
mod diff;
mod failure;
mod hydration;
mod identity;
mod inflight;
mod library;
mod nested;
mod own_removals;
mod placeholder;
mod population;
mod reconcile;
mod reconciliation;
mod refresh;
mod reporting;
mod write_guard;

#[cfg(windows)]
mod adapter;

pub(crate) use activation::ActivationError;
#[cfg(test)]
pub(crate) use activation::{NESTED_SYNC_ROOT_ERROR, START_FAILED_ERROR};
#[cfg(test)]
pub(crate) use cleanup::CLEANUP_FAILED_ERROR;
pub(crate) use cleanup::{CleanupError, CleanupReport};
#[cfg(test)]
pub(crate) use nested::{NestedRoot, Relation as NestedRelation};
pub(crate) use reconciliation::{mark_populated, run_schedule, LibraryRefresh, SystemRefresh};
pub(crate) use reporting::{install as install_notice_sink, Notice, NoticeKind, NoticeSink};

use std::path::PathBuf;

/// The provider part of the sync root id (`<provider>!<SID>!<account>`), also how Lymic recognises
/// its own registrations.
const PROVIDER_ID: &str = "Lymic.VirtualLibrary";

/// Starts and stops serving the Virtual Library, behind a trait so the settings commands are
/// testable without Windows.
pub(crate) trait SyncRootService: Sync {
    /// Checks the folder against other sync roots, registers it and starts serving it. Must be
    /// called from within the Tokio runtime: it is the runtime the callbacks bridge to.
    fn start(
        &self,
        root: PathBuf,
    ) -> impl std::future::Future<Output = Result<(), ActivationError>> + Send;

    /// Stops serving callbacks only. The registration and the placeholders stay; used when a
    /// start has to be undone. Disabling and logout use [`SyncRootService::clean_up`].
    fn stop(&self);

    /// Stops serving, unregisters the sync root and removes everything Lymic created in `root`
    /// (`cleanup`). Works when nothing is connected or registered. Files Lymic did not create stay,
    /// with their folder; the report says what is left.
    fn clean_up(
        &self,
        root: PathBuf,
    ) -> impl std::future::Future<Output = Result<CleanupReport, CleanupError>> + Send;
}

/// The real thing: Windows only.
pub(crate) struct SystemSyncRoot;

impl SyncRootService for SystemSyncRoot {
    #[cfg(windows)]
    async fn start(&self, root: PathBuf) -> Result<(), ActivationError> {
        adapter::start(root).await
    }

    #[cfg(not(windows))]
    async fn start(&self, _root: PathBuf) -> Result<(), ActivationError> {
        Err(ActivationError::Unsupported)
    }

    fn stop(&self) {
        #[cfg(windows)]
        adapter::stop();
    }

    #[cfg(windows)]
    async fn clean_up(&self, root: PathBuf) -> Result<CleanupReport, CleanupError> {
        adapter::clean_up(root).await
    }

    #[cfg(not(windows))]
    async fn clean_up(&self, _root: PathBuf) -> Result<CleanupReport, CleanupError> {
        Err(CleanupError::Unsupported)
    }
}
