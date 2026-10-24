mod callback;
mod ingestion;
mod state;

pub use callback::create_watcher;
pub(crate) use ingestion::{WatcherIngestion, WatcherInput, WATCHER_DEBOUNCE};
pub use state::{unwatch_path, watch_path, WatchRegistration, WatcherState};
