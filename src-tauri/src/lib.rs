mod app;
mod audit;
mod auth;
mod commands;
mod db;
mod immich;
mod media;
pub(crate) mod removable;
mod sync;
// The whole projection (and `immich::library_listing`, which only it calls) is unused outside its
// tests until the Windows adapter (ADR-0003, Phase 2) consumes it; the settings surface (#272) has
// landed but does not read it. Remove this allow when the adapter lands.
#[allow(dead_code)]
mod virtual_library;
mod virtual_library_root;
mod watched_folders;
mod watcher;

pub fn run() {
    app::run();
}
