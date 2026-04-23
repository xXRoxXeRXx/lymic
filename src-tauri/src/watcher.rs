use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;

pub fn create_watcher(tx: mpsc::Sender<Event>) -> notify::Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        let _ = tx.blocking_send(event);
                    }
                    _ => {}
                }
            }
        },
        Config::default(),
    )?;

    Ok(watcher)
}

pub fn watch_path(watcher: &mut RecommendedWatcher, path: &str) -> notify::Result<()> {
    let p = Path::new(path);
    if p.exists() {
        watcher.watch(p, RecursiveMode::Recursive)?;
    }
    Ok(())
}

pub fn unwatch_path(watcher: &mut RecommendedWatcher, path: &str) -> notify::Result<()> {
    watcher.unwatch(Path::new(path))?;
    Ok(())
}
