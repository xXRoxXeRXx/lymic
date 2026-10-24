use super::WatcherInput;
use notify::{Config, Event, EventKind, RecommendedWatcher, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};

pub(crate) fn send_or_request_rescan(
    tx: &mpsc::Sender<WatcherInput>,
    message: WatcherInput,
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

pub(crate) fn dispatch_event(
    event: Event,
    tx: &mpsc::Sender<WatcherInput>,
    rescan_requested: &AtomicBool,
    rescan_notify: &Notify,
) {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            // A full queue means individual paths are no longer trustworthy. The
            // receiver will reconcile every watched folder after draining its batch.
            send_or_request_rescan(
                tx,
                WatcherInput::Event(event),
                rescan_requested,
                rescan_notify,
            );
        }
        EventKind::Any | EventKind::Other => {
            if tx.try_send(WatcherInput::Overflow).is_err() {
                request_rescan(rescan_requested, rescan_notify);
            }
        }
        EventKind::Remove(_) | EventKind::Access(_) => {}
    }
}

pub fn create_watcher(
    tx: mpsc::Sender<WatcherInput>,
    rescan_requested: Arc<AtomicBool>,
    rescan_notify: Arc<Notify>,
) -> notify::Result<RecommendedWatcher> {
    RecommendedWatcher::new(
        move |res: notify::Result<Event>| match res {
            Ok(event) => dispatch_event(event, &tx, &rescan_requested, &rescan_notify),
            Err(_) => {
                // notify itself can report an overflow or backend error. Reconcile rather
                // than silently accepting a potentially incomplete event stream.
                if tx.try_send(WatcherInput::Overflow).is_err() {
                    request_rescan(&rescan_requested, &rescan_notify);
                }
            }
        },
        Config::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    use std::path::PathBuf;

    async fn was_notified(notify: &Notify) -> bool {
        tokio::time::timeout(std::time::Duration::ZERO, notify.notified())
            .await
            .is_ok()
    }

    #[tokio::test]
    async fn create_and_modify_events_are_forwarded_unchanged() {
        let (tx, mut rx) = mpsc::channel(2);
        let dirty = AtomicBool::new(false);
        let notify = Notify::new();
        let create =
            Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from("created.jpg"));
        let rename = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("before.jpg"))
            .add_path(PathBuf::from("after.jpg"));

        dispatch_event(create.clone(), &tx, &dirty, &notify);
        dispatch_event(rename.clone(), &tx, &dirty, &notify);

        assert!(matches!(rx.recv().await, Some(WatcherInput::Event(event)) if event == create));
        assert!(matches!(rx.recv().await, Some(WatcherInput::Event(event)) if event == rename));
        assert!(!dirty.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn ambiguous_events_request_a_rescan_without_queueing() {
        for kind in [EventKind::Any, EventKind::Other] {
            let (tx, mut rx) = mpsc::channel(1);
            let dirty = AtomicBool::new(false);
            let notify = Notify::new();

            dispatch_event(Event::new(kind), &tx, &dirty, &notify);

            assert!(matches!(rx.recv().await, Some(WatcherInput::Overflow)));
            assert!(!dirty.load(Ordering::Acquire));
            assert!(!was_notified(&notify).await);
        }
    }

    #[tokio::test]
    async fn remove_events_are_ignored() {
        let (tx, mut rx) = mpsc::channel(1);
        let dirty = AtomicBool::new(false);
        let notify = Notify::new();

        dispatch_event(
            Event::new(EventKind::Remove(RemoveKind::File)).add_path(PathBuf::from("gone.jpg")),
            &tx,
            &dirty,
            &notify,
        );

        assert!(!dirty.load(Ordering::Acquire));
        assert!(rx.try_recv().is_err());
        assert!(!was_notified(&notify).await);
    }

    #[tokio::test]
    async fn full_channel_requests_a_rescan() {
        let (tx, _rx) = mpsc::channel(1);
        let dirty = AtomicBool::new(false);
        let notify = Notify::new();

        tx.try_send(WatcherInput::Overflow).unwrap();
        send_or_request_rescan(&tx, WatcherInput::Overflow, &dirty, &notify);

        assert!(dirty.load(Ordering::Acquire));
        assert!(was_notified(&notify).await);
    }
}
