mod app;
mod audit;
mod auth;
mod commands;
mod db;
mod immich;
mod media;
pub(crate) mod removable;
mod sync;
pub(crate) mod virtual_library;
mod watcher;

pub fn run() {
    app::run();
}
