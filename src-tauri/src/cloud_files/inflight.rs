//! Hydrations in flight, so a cancelled read stops its download (ADR-0003, #281).
//!
//! When the program that opened a placeholder gives up (closes the file, the I/O times out), the
//! platform calls `CANCEL_FETCH_DATA` for it. The download is a spawned task that would otherwise
//! run to the end, pulling a whole original from Immich for nobody. Each hydration registers under
//! the key of its transfer; a cancel for that key drops the download mid-stream, which closes the
//! connection to Immich.
//!
//! A hydration always fetches the whole original, so several fetches for one file (different
//! ranges) are redundant and cancelling all of them is safe: the platform asks again for what a
//! live reader still needs.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

type Entries = HashMap<i64, Vec<(u64, CancellationToken)>>;

/// The hydrations in flight, by the platform's transfer key.
#[derive(Default)]
pub(super) struct InFlight {
    entries: Arc<Mutex<Entries>>,
    next_id: Mutex<u64>,
}

/// One registered hydration. Dropping it unregisters it.
pub(super) struct Registration {
    key: i64,
    id: u64,
    token: CancellationToken,
    entries: Arc<Mutex<Entries>>,
}

impl InFlight {
    pub(super) fn begin(&self, key: i64) -> Registration {
        let id = {
            let mut next = self.next_id.lock().expect("id lock");
            *next += 1;
            *next
        };
        let token = CancellationToken::new();
        self.entries
            .lock()
            .expect("entries lock")
            .entry(key)
            .or_default()
            .push((id, token.clone()));
        Registration {
            key,
            id,
            token,
            entries: self.entries.clone(),
        }
    }

    /// Cancels every hydration in flight under `key`; returns how many there were.
    pub(super) fn cancel(&self, key: i64) -> usize {
        let tokens: Vec<CancellationToken> = self
            .entries
            .lock()
            .expect("entries lock")
            .get(&key)
            .map(|found| found.iter().map(|(_, token)| token.clone()).collect())
            .unwrap_or_default();
        for token in &tokens {
            token.cancel();
        }
        tokens.len()
    }
}

impl Registration {
    /// Runs `work` until it ends or the hydration is cancelled. `None` means cancelled: `work` is
    /// dropped where it stood, so whatever it held (the HTTP response) is closed.
    pub(super) async fn run<F: Future>(&self, work: F) -> Option<F::Output> {
        tokio::select! {
            biased;
            () = self.token.cancelled() => None,
            output = work => Some(output),
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(found) = entries.get_mut(&self.key) {
                found.retain(|(id, _)| *id != self.id);
                if found.is_empty() {
                    entries.remove(&self.key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Sets a flag when dropped, to see that the work was dropped and not merely abandoned.
    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_cancel_stops_the_download_it_names_and_drops_its_work() {
        let in_flight = Arc::new(InFlight::default());
        let dropped = Arc::new(AtomicBool::new(false));
        let registration = in_flight.begin(7);
        let flag = DropFlag(dropped.clone());
        let download = tokio::spawn(async move {
            registration
                .run(async move {
                    let _held = flag;
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                })
                .await
        });
        tokio::task::yield_now().await;

        assert_eq!(in_flight.cancel(7), 1);

        assert_eq!(download.await.unwrap(), None);
        assert!(dropped.load(Ordering::SeqCst), "the download was dropped");
    }

    #[tokio::test]
    async fn a_cancel_leaves_other_transfers_alone() {
        let in_flight = InFlight::default();
        let other = in_flight.begin(8);
        let _mine = in_flight.begin(7);

        assert_eq!(in_flight.cancel(7), 1);

        assert_eq!(other.run(async { 42 }).await, Some(42));
    }

    #[tokio::test]
    async fn a_finished_hydration_is_unregistered_so_a_late_cancel_finds_nothing() {
        let in_flight = InFlight::default();
        let registration = in_flight.begin(7);
        assert_eq!(registration.run(async { "done" }).await, Some("done"));
        drop(registration);

        assert_eq!(in_flight.cancel(7), 0);
    }

    #[tokio::test]
    async fn every_fetch_of_the_cancelled_transfer_is_cancelled() {
        let in_flight = InFlight::default();
        let first = in_flight.begin(7);
        let second = in_flight.begin(7);

        assert_eq!(in_flight.cancel(7), 2);

        assert_eq!(first.run(std::future::pending::<()>()).await, None);
        assert_eq!(second.run(std::future::pending::<()>()).await, None);
    }
}
