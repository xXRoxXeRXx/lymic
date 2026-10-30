mod app;
mod audit;
mod auth;
mod commands;
mod db;
mod immich;
mod media;
pub(crate) mod removable;
mod sync;
mod watched_folders;
mod watcher;

pub fn run() {
    app::run();
}
