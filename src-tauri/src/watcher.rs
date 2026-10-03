use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Notify;

pub(crate) fn send_or_request_rescan<T>(
    tx: &mpsc::Sender<T>,
    message: T,
    rescan_requested: &AtomicBool,
    rescan_notify: &Notify,
) {
    if tx.try_send(message).is_err() {
        request_rescan(rescan_requested, rescan_notify);
    }
}

pub(crate) fn request_rescan(rescan_requested: &AtomicBool, rescan_notify: &Notify) {
    rescan_requested.store(true, Ordering::Release);
    // Notify retains a permit when the receiver is idle, so a late overflow
    // cannot leave a dirty flag without a future reconciliation trigger.
    rescan_notify.notify_one();
}

pub fn create_watcher(
    tx: mpsc::Sender<Event>,
    rescan_requested: Arc<AtomicBool>,
    rescan_notify: Arc<Notify>,
) -> notify::Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        // A full queue means individual paths are no longer trustworthy. The
                        // receiver will reconcile every watched folder after draining its batch.
                        send_or_request_rescan(&tx, event, &rescan_requested, &rescan_notify);
                    }
                    _ => {}
                },
                Err(_) => {
                    // notify itself can report an overflow or backend error. Reconcile rather
                    // than silently accepting a potentially incomplete event stream.
                    request_rescan(&rescan_requested, &rescan_notify);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_channel_requests_a_rescan() {
        let (tx, _rx) = mpsc::channel(1);
        let dirty = AtomicBool::new(false);
        let notify = Notify::new();

        tx.try_send(1).unwrap();
        send_or_request_rescan(&tx, 2, &dirty, &notify);

        assert!(dirty.load(Ordering::Acquire));
        assert!(tokio::time::timeout(std::time::Duration::ZERO, notify.notified())
            .await
            .is_ok());
    }
}
