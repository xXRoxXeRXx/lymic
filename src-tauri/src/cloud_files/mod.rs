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
//! Lifecycle in this ticket: [`SystemSyncRoot::start`] runs when the user enables the Virtual
//! Library and again on app start while it is enabled; [`SystemSyncRoot::stop`] only stops serving
//! callbacks. Unregistering the sync root and removing what Lymic created is #283.
//!
//! Hooks for the follow-up tickets: #279 adds the delete and rename vetoes to the callback table in
//! `adapter::connection`; #280 compares what `population::populate` plans with the directories
//! already populated; #281 refines `failure::Failure::status`, retries and the in-app notice.

// Outside Windows only the tests use most of this module.
#![cfg_attr(not(windows), allow(dead_code))]

mod activation;
mod failure;
mod hydration;
mod identity;
mod library;
mod nested;
mod placeholder;
mod population;

#[cfg(windows)]
mod adapter;

pub(crate) use activation::ActivationError;
#[cfg(test)]
pub(crate) use activation::{NESTED_SYNC_ROOT_ERROR, START_FAILED_ERROR};
#[cfg(test)]
pub(crate) use nested::{NestedRoot, Relation as NestedRelation};

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

    /// Stops serving callbacks. Registered sync roots and placeholders stay (#283).
    fn stop(&self);
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
}
