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
use super::diagnostic_log::SystemNotes;
use super::nested::{RegisteredRoot, SyncRootProbe};
use super::shell_thumbnails::{self, ItemRefresh};
use super::watchdog;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// The runtime the callbacks spawn their work onto.
static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();
/// The live connection, if any. Dropping the sync root's connection stops callbacks.
static CONNECTION: Mutex<Option<callbacks::Connection>> = Mutex::new(None);

/// The root being served, `None` once the Virtual Library is stopped on purpose. The watchdog only
/// repairs a connection while there is one.
static SERVED_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Looks after the connection while it is served.
static WATCHDOG: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

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

pub(super) fn note(message: &str) {
    callbacks::note(message);
}

fn runtime() -> Option<&'static tokio::runtime::Handle> {
    RUNTIME.get()
}

/// Registers and connects `root`. Must be awaited inside the Tokio runtime.
pub(super) async fn start(root: std::path::PathBuf) -> Result<(), ActivationError> {
    callbacks::log_panics();
    RUNTIME.get_or_init(tokio::runtime::Handle::current);
    note(&format!(
        "starting the Virtual Library at '{}'",
        root.display()
    ));
    let started = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || activation::activate(&root, &WindowsHost))
            .await
            .map_err(|error| ActivationError::Connect(error.to_string()))?
    };
    match &started {
        Ok(()) => {
            note(&format!(
                "the Virtual Library is served at '{}'",
                root.display()
            ));
            watch_connection();
        }
        Err(error) => note(&format!(
            "the Virtual Library could not be started at '{}': {error:?}",
            root.display()
        )),
    }
    started
}

/// Starts the watchdog, once, on the application's runtime.
fn watch_connection() {
    let (Some(runtime), Ok(mut slot)) = (runtime(), WATCHDOG.lock()) else {
        return;
    };
    if slot.as_ref().is_some_and(|task| !task.is_finished()) {
        return;
    }
    *slot = Some(runtime.spawn(watchdog::run(
        Arc::new(ConnectionWatch),
        Arc::new(SystemNotes),
        watchdog::INTERVAL,
    )));
}

/// The connection as the watchdog sees it.
struct ConnectionWatch;

impl watchdog::Provider for ConnectionWatch {
    fn check(&self) -> Result<(), String> {
        let root = SERVED_ROOT
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or_else(|| "the Virtual Library is stopped".to_string())?;
        match CONNECTION.lock() {
            Ok(slot) => match slot.as_ref() {
                None => return Err("there is no connection to the sync root".to_string()),
                Some(connection) => connection.heartbeat().map_err(|error| {
                    format!("the platform no longer knows the connection: {error}")
                })?,
            },
            Err(_) => return Err("the connection record is poisoned".to_string()),
        }
        match probe::provider_at(&root) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!(
                "'{}' is not a registered sync root",
                root.display()
            )),
            Err(error) => Err(error),
        }
    }

    fn reconnect(&self) -> Result<(), String> {
        let root = SERVED_ROOT
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or_else(|| "the Virtual Library was stopped meanwhile".to_string())?;
        WindowsHost.register(&root)?;
        WindowsHost.connect(&root)
    }
}

/// The Virtual Library folder on disk, for reconciliation (#280).
pub(super) fn disk_tree(root: std::path::PathBuf, populated: &[String]) -> reconcile::DiskTree {
    reconcile::DiskTree::new(root, populated)
}

/// Stops serving callbacks for good: the watchdog stops too. The registration and the placeholders
/// stay.
pub(super) fn stop() {
    note("stopping the Virtual Library");
    if let Ok(mut slot) = SERVED_ROOT.lock() {
        *slot = None;
    }
    if let Some(watchdog) = WATCHDOG.lock().ok().and_then(|mut slot| slot.take()) {
        watchdog.abort();
    }
    stop_serving();
}

/// Drops the connection and the thumbnail request server, to connect again or to stop.
fn stop_serving() {
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
        stop_serving();
        let connection = callbacks::connect(root).map_err(|error| error.to_string())?;
        if let Ok(mut slot) = CONNECTION.lock() {
            *slot = Some(connection);
        }
        if let Ok(mut slot) = SERVED_ROOT.lock() {
            *slot = Some(root.to_path_buf());
        }
        if let (Some(runtime), Ok(mut slot)) = (runtime(), MISS_SERVER.lock()) {
            *slot = Some(runtime.spawn(shell_thumbnails::run_miss_server(ShellRefresh)));
        }
        Ok(())
    }
}
