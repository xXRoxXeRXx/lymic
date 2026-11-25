mod app;
mod audit;
mod auth;
mod cloud_files;
mod commands;
mod db;
mod immich;
mod media;
pub(crate) mod removable;
mod sync;
mod virtual_library;
mod virtual_library_root;
mod watched_folders;
mod watcher;

pub fn run() {
    app::run();
}
