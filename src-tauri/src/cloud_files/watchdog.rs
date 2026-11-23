//! Keeping the sync root connected (ADR-0003, #266).
//!
//! Explorer shows the placeholders for as long as they exist on disk, but a click on one only works
//! while Lymic holds a connection to the sync root. When that connection is gone (never made,
//! dropped, or the registration vanished) Explorer says "Make sure Lymic is running on your PC"
//! (`ERROR_CLOUD_FILE_PROVIDER_NOT_RUNNING`) although the app runs, and none of the callbacks ever
//! reaches the log. The watchdog looks at the connection every [`INTERVAL`] and connects again when
//! it is not there, saying so in the diagnostic log either way.
//!
//! The decision is platform-neutral and tested here; the adapter supplies the Windows calls
//! ([`Provider`]).

use super::diagnostic_log::{Notes, SharedNotes};
use std::sync::Arc;
use std::time::Duration;

/// How often the connection is looked at.
pub(super) const INTERVAL: Duration = Duration::from_secs(30);

/// The connection to the sync root, as the watchdog needs it. Both calls block.
pub(super) trait Provider: Send + Sync + 'static {
    /// `Ok` when callbacks can still reach the app, otherwise why not.
    fn check(&self) -> Result<(), String>;

    /// Registers and connects the sync root again, replacing whatever is left of the old one.
    fn reconnect(&self) -> Result<(), String>;
}

/// What one look at the connection did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Connected,
    Reconnected,
    ReconnectFailed,
}

/// Looks at the connection once and repairs it when it is lost.
pub(super) fn check_once(provider: &impl Provider, notes: &dyn Notes) -> Outcome {
    let Err(reason) = provider.check() else {
        return Outcome::Connected;
    };
    notes.note(&format!(
        "the sync root connection is lost ({reason}); connecting again"
    ));
    match provider.reconnect() {
        Ok(()) => {
            notes.note("the sync root is connected again");
            Outcome::Reconnected
        }
        Err(error) => {
            notes.note(&format!("connecting the sync root again failed: {error}"));
            Outcome::ReconnectFailed
        }
    }
}

/// Looks at the connection every `interval` until the task is aborted. A failed repair is tried
/// again at the next look.
pub(super) async fn run<P: Provider>(provider: Arc<P>, notes: SharedNotes, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let (provider, notes) = (provider.clone(), notes.clone());
        // The platform calls block, and a panic in them must not end the watching.
        if let Err(error) =
            tokio::task::spawn_blocking(move || check_once(&*provider, &*notes)).await
        {
            super::system_note(&format!("the sync root watchdog stopped a check: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorded(Mutex<Vec<String>>);

    impl Notes for Recorded {
        fn note(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    impl Recorded {
        fn lines(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct FakeProvider {
        alive: Mutex<Result<(), String>>,
        reconnect_result: Mutex<Result<(), String>>,
        reconnects: AtomicUsize,
    }

    impl FakeProvider {
        fn new(alive: Result<(), String>, reconnect: Result<(), String>) -> Self {
            Self {
                alive: Mutex::new(alive),
                reconnect_result: Mutex::new(reconnect),
                reconnects: AtomicUsize::new(0),
            }
        }
    }

    impl Provider for FakeProvider {
        fn check(&self) -> Result<(), String> {
            self.alive.lock().unwrap().clone()
        }

        fn reconnect(&self) -> Result<(), String> {
            self.reconnects.fetch_add(1, Ordering::SeqCst);
            let result = self.reconnect_result.lock().unwrap().clone();
            if result.is_ok() {
                *self.alive.lock().unwrap() = Ok(());
            }
            result
        }
    }

    #[test]
    fn a_connection_that_is_there_is_left_alone_and_nothing_is_logged() {
        let provider = FakeProvider::new(Ok(()), Ok(()));
        let notes = Recorded::default();

        assert_eq!(check_once(&provider, &notes), Outcome::Connected);

        assert_eq!(provider.reconnects.load(Ordering::SeqCst), 0);
        assert!(notes.lines().is_empty());
    }

    #[test]
    fn a_lost_connection_is_made_again_and_both_steps_are_logged() {
        let provider = FakeProvider::new(Err("no connection".into()), Ok(()));
        let notes = Recorded::default();

        assert_eq!(check_once(&provider, &notes), Outcome::Reconnected);

        assert_eq!(provider.reconnects.load(Ordering::SeqCst), 1);
        let lines = notes.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("no connection"), "{lines:?}");
        assert!(lines[1].contains("connected again"), "{lines:?}");
    }

    #[test]
    fn a_reconnect_that_fails_is_logged_with_its_reason() {
        let provider = FakeProvider::new(Err("gone".into()), Err("0x80070005".into()));
        let notes = Recorded::default();

        assert_eq!(check_once(&provider, &notes), Outcome::ReconnectFailed);

        assert!(
            notes.lines().iter().any(|line| line.contains("0x80070005")),
            "{:?}",
            notes.lines()
        );
    }

    #[tokio::test]
    async fn the_watchdog_keeps_looking_until_the_repair_works() {
        let provider = Arc::new(FakeProvider::new(Err("gone".into()), Err("busy".into())));
        let notes = Arc::new(Recorded::default());
        let watching = tokio::spawn(run(
            provider.clone(),
            notes.clone(),
            Duration::from_millis(10),
        ));
        for _ in 0..200 {
            if provider.reconnects.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The next repair works.
        *provider.reconnect_result.lock().unwrap() = Ok(());
        for _ in 0..200 {
            if notes
                .lines()
                .iter()
                .any(|line| line.contains("connected again"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        watching.abort();

        assert!(provider.reconnects.load(Ordering::SeqCst) >= 3);
        assert!(notes
            .lines()
            .iter()
            .any(|line| line.contains("connected again")));
    }
}
