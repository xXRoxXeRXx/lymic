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
mod teardown;

use super::activation::{self, ActivationError, SyncRootHost};
use super::nested::{RegisteredRoot, SyncRootProbe};
use super::shell_thumbnails::{self, ItemRefresh};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// The runtime the callbacks spawn their work onto.
static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();
/// The live connection, if any. Dropping the sync root's connection stops callbacks.
static CONNECTION: Mutex<Option<callbacks::Connection>> = Mutex::new(None);

/// Serves the thumbnails Explorer asked for while the sync root is connected (`shell_thumbnails`).
static MISS_SERVER: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

/// Tells the shell that an item changed (`SHChangeNotify`), so Explorer asks for its thumbnail again.
struct ShellRefresh;

impl ItemRefresh for ShellRefresh {
    fn refresh(&self, path: &str) {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL terminated path that outlives the call; the second item is unused
        // for this event.
        unsafe {
            windows_sys::Win32::UI::Shell::SHChangeNotify(
                windows_sys::Win32::UI::Shell::SHCNE_UPDATEITEM as i32,
                windows_sys::Win32::UI::Shell::SHCNF_PATHW,
                wide.as_ptr().cast(),
                std::ptr::null(),
            );
        }
    }
}

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
    if let Some(server) = MISS_SERVER.lock().ok().and_then(|mut slot| slot.take()) {
        server.abort();
    }
    let connection = CONNECTION.lock().ok().and_then(|mut slot| slot.take());
    if let Some(connection) = connection {
        connection.disconnect();
    }
}

/// Disconnects, unregisters the sync root and removes what Lymic created in `root`
/// (`cloud_files::cleanup`). Blocking work runs on a blocking thread.
pub(super) async fn clean_up(
    root: std::path::PathBuf,
) -> Result<super::cleanup::CleanupReport, super::cleanup::CleanupError> {
    let result = tokio::task::spawn_blocking(move || {
        super::cleanup::clean_up(&root, &teardown::WindowsCleanup)
    })
    .await
    .map_err(|error| super::cleanup::CleanupError::Scan(error.to_string()))?;
    // Whatever the outcome, no thumbnail of the account stays behind (#282).
    super::shell_thumbnails::clear_system_cache();
    result
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
        if let (Some(runtime), Ok(mut slot)) = (runtime(), MISS_SERVER.lock()) {
            *slot = Some(runtime.spawn(shell_thumbnails::run_miss_server(ShellRefresh)));
        }
        Ok(())
    }
}
