mod app;
mod audit;
mod auth;
mod commands;
mod db;
mod immich;
mod media;
mod sync;
mod watcher;

pub fn run() {
    app::run();
}
