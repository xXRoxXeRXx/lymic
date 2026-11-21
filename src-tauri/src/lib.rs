mod app;
mod audit;
mod auth;
mod commands;
mod db;
mod immich;
mod media;
pub(crate) mod removable;
mod sync;
// The projection has no caller outside its tests until the Windows adapter (ADR-0003, Phase 2) and
// the settings surface (#272) consume it; `immich::library_listing` is its only other user.
#[allow(dead_code)]
mod virtual_library;
mod watched_folders;
mod watcher;

pub fn run() {
    app::run();
}
