//! Hydrations in flight, so a cancelled read stops its download (ADR-0003, #281).
//!
//! When the program that opened a placeholder gives up (closes the file, the I/O times out), the
//! platform calls `CANCEL_FETCH_DATA` for it. The download is a spawned task that would otherwise
//! run to the end, pulling a whole original from Immich for nobody. Each hydration registers under
//! the key of its transfer; a cancel for that key drops the download mid-stream, which closes the
//! connection to Immich.
//!
//! Under the progressive policy one file has several fetches at once, one per range a reader waits
//! for (a program reads the start and a later page of the same file in parallel). The platform names
//! the range it cancels, and only the fetches that overlap it are stopped: stopping the others would
//! leave a read that is still waiting without an answer.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// One fetch: its id, the range it serves (`length` 0 or less means "to the end") and its token.
type Fetch = (u64, (i64, i64), CancellationToken);
type Entries = HashMap<i64, Vec<Fetch>>;

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
    pub(super) fn begin(&self, key: i64, offset: i64, length: i64) -> Registration {
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
            .push((id, (offset, length), token.clone()));
        Registration {
            key,
            id,
            token,
            entries: self.entries.clone(),
        }
    }

    /// Cancels the hydrations in flight under `key` that serve the range `offset`/`length` the
    /// platform cancelled; returns how many there were. When none overlaps it (the platform spelled
    /// the range differently), every hydration of the transfer is cancelled, as the platform asks
    /// again for what a live reader still needs.
    pub(super) fn cancel(&self, key: i64, offset: i64, length: i64) -> usize {
        let tokens: Vec<CancellationToken> = {
            let entries = self.entries.lock().expect("entries lock");
            let found = entries.get(&key).map(Vec::as_slice).unwrap_or_default();
            let overlapping: Vec<_> = found
                .iter()
                .filter(|(_, range, _)| overlaps(*range, (offset, length)))
                .collect();
            let chosen = if overlapping.is_empty() {
                found.iter().collect()
            } else {
                overlapping
            };
            chosen.iter().map(|(_, _, token)| token.clone()).collect()
        };
        for token in &tokens {
            token.cancel();
        }
        tokens.len()
    }
}

/// Whether two `(offset, length)` ranges share a byte. A length of 0 or less reaches the end.
fn overlaps(a: (i64, i64), b: (i64, i64)) -> bool {
    let end = |(offset, length): (i64, i64)| {
        if length <= 0 {
            i64::MAX
        } else {
            offset.saturating_add(length)
        }
    };
    a.0 < end(b) && b.0 < end(a)
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
                found.retain(|(id, _, _)| *id != self.id);
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
        let registration = in_flight.begin(7, 0, 4096);
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

        assert_eq!(in_flight.cancel(7, 0, 4096), 1);

        assert_eq!(download.await.unwrap(), None);
        assert!(dropped.load(Ordering::SeqCst), "the download was dropped");
    }

    #[tokio::test]
    async fn a_cancel_leaves_other_transfers_alone() {
        let in_flight = InFlight::default();
        let other = in_flight.begin(8, 0, 4096);
        let _mine = in_flight.begin(7, 0, 4096);

        assert_eq!(in_flight.cancel(7, 0, 4096), 1);

        assert_eq!(other.run(async { 42 }).await, Some(42));
    }

    #[tokio::test]
    async fn a_finished_hydration_is_unregistered_so_a_late_cancel_finds_nothing() {
        let in_flight = InFlight::default();
        let registration = in_flight.begin(7, 0, 4096);
        assert_eq!(registration.run(async { "done" }).await, Some("done"));
        drop(registration);

        assert_eq!(in_flight.cancel(7, 0, 4096), 0);
    }

    #[tokio::test]
    async fn every_fetch_of_the_cancelled_transfer_is_cancelled() {
        let in_flight = InFlight::default();
        let first = in_flight.begin(7, 0, 4096);
        let second = in_flight.begin(7, 0, 4096);

        assert_eq!(in_flight.cancel(7, 0, 4096), 2);

        assert_eq!(first.run(std::future::pending::<()>()).await, None);
        assert_eq!(second.run(std::future::pending::<()>()).await, None);
    }

    #[tokio::test]
    async fn a_cancel_stops_only_the_fetch_of_its_range_and_leaves_the_other_reader_waiting_for_data(
    ) {
        let in_flight = InFlight::default();
        let start = in_flight.begin(7, 0, 65_536);
        let later = in_flight.begin(7, 1_048_576, 4_096);

        assert_eq!(in_flight.cancel(7, 1_048_576, 4_096), 1);

        assert_eq!(later.run(std::future::pending::<()>()).await, None);
        assert_eq!(
            start.run(async { "still wanted" }).await,
            Some("still wanted")
        );
    }

    #[tokio::test]
    async fn a_cancel_whose_range_matches_no_fetch_stops_every_fetch_of_the_transfer() {
        let in_flight = InFlight::default();
        let first = in_flight.begin(7, 0, 4_096);
        let second = in_flight.begin(7, 8_192, 4_096);

        assert_eq!(in_flight.cancel(7, 100_000_000, 4_096), 2);

        assert_eq!(first.run(std::future::pending::<()>()).await, None);
        assert_eq!(second.run(std::future::pending::<()>()).await, None);
    }

    #[test]
    fn ranges_overlap_by_a_shared_byte_and_an_open_end_reaches_everything_after_it() {
        assert!(overlaps((0, 10), (9, 5)));
        assert!(!overlaps((0, 10), (10, 5)));
        assert!(overlaps((100, 0), (1_000_000, 4_096)));
        assert!(!overlaps((100, 0), (0, 100)));
    }
}
