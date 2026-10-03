use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;

pub fn create_watcher(tx: mpsc::UnboundedSender<Event>) -> notify::Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        // The receiver batches and deduplicates events. An unbounded channel
                        // prevents filesystem changes from being silently lost under load.
                        let _ = tx.send(event);
                    }
                    _ => {}
                }
            }
        },
        Config::default(),
    )?;

    Ok(watcher)
}

pub fn watch_path(watcher: &mut RecommendedWatcher, path: &str) -> notify::Result<bool> {
    let p = Path::new(path);
    if p.exists() {
        watcher.watch(p, RecursiveMode::Recursive)?;
        Ok(true)
    } else {
        // path doesn't exist (e.g. removable drive offline).
        // Return Ok(false) so callers can warn the user instead of silently
        // showing the folder as "watched" when events will never fire.
        Ok(false)
    }
}

pub fn unwatch_path(watcher: &mut RecommendedWatcher, path: &str) -> notify::Result<()> {
    watcher.unwatch(Path::new(path))?;
    Ok(())
}
