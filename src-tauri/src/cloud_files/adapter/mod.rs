//! The Windows calls: sync root registration, the nested-root probes and the Cloud Filter callbacks.
//! Nothing here decides anything about the projection; it forwards to the platform-neutral parts of
//! `cloud_files`. It cannot be unit tested, so it stays small and is covered by
//! `docs/virtual-library-verification.md`.
//!
//! Threading: the Cloud Filter platform calls the callbacks on its own OS threads. A callback only
//! copies what it needs out of the platform's structures and spawns the work onto the application's
//! Tokio runtime, then returns; the work completes the request later through `CfExecute`, which is
//! allowed from any thread. No callback blocks on the network, and nothing here runs on the UI
//! thread: registration runs on a blocking worker thread.

mod callbacks;
mod probe;
mod reconcile;
mod register;

use super::activation::{self, ActivationError, SyncRootHost};
use super::nested::{RegisteredRoot, SyncRootProbe};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// The runtime the callbacks spawn their work onto.
static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();
/// The live connection, if any. Dropping the sync root's connection stops callbacks.
static CONNECTION: Mutex<Option<callbacks::Connection>> = Mutex::new(None);

fn runtime() -> Option<&'static tokio::runtime::Handle> {
    RUNTIME.get()
}

/// Registers and connects `root`. Must be awaited inside the Tokio runtime.
pub(super) async fn start(root: std::path::PathBuf) -> Result<(), ActivationError> {
    RUNTIME.get_or_init(tokio::runtime::Handle::current);
    tokio::task::spawn_blocking(move || activation::activate(&root, &WindowsHost))
        .await
        .map_err(|error| ActivationError::Connect(error.to_string()))?
}

/// The Virtual Library folder on disk, for reconciliation (#280).
pub(super) fn disk_tree(root: std::path::PathBuf, populated: &[String]) -> reconcile::DiskTree {
    reconcile::DiskTree::new(root, populated)
}

/// Stops serving callbacks. The registration and the placeholders stay.
pub(super) fn stop() {
    let connection = CONNECTION.lock().ok().and_then(|mut slot| slot.take());
    if let Some(connection) = connection {
        connection.disconnect();
    }
}

struct WindowsHost;

impl SyncRootProbe for WindowsHost {
    fn provider_at(&self, path: &Path) -> Result<Option<String>, String> {
        probe::provider_at(path)
    }

    fn registered_roots(&self) -> Result<Vec<RegisteredRoot>, String> {
        probe::registered_roots()
    }
}

impl SyncRootHost for WindowsHost {
    fn create_root_folder(&self, root: &Path) -> Result<(), String> {
        std::fs::create_dir_all(root).map_err(|error| error.to_string())
    }

    fn register(&self, root: &Path) -> Result<(), String> {
        register::register(root).map_err(|error| error.to_string())
    }

    fn connect(&self, root: &Path) -> Result<(), String> {
        // A second connection to the same root is refused, so serve through a fresh one.
        stop();
        let connection = callbacks::connect(root).map_err(|error| error.to_string())?;
        if let Ok(mut slot) = CONNECTION.lock() {
            *slot = Some(connection);
        }
        Ok(())
    }
}
