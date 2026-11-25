//! When reconciliation runs, and the guarantee that two runs never overlap (ADR-0003, #280).
//!
//! Three things start a run: the application start, a 10 minute timer and the Refresh Library
//! button. They all go through one [`SingleFlight`]:
//!
//! * A start or timer run that finds a run already in progress does nothing. The run in progress
//!   already covers it, and the next tick comes in ten minutes.
//! * A manual refresh that finds a run in progress **waits for it and then runs once more**, because
//!   the run in progress may already have passed the directory the user just changed (the usual
//!   reason to press the button is a photo uploaded a moment ago). Several manual refreshes that
//!   arrive during the same run share that one follow-up run and all get its result; nobody queues
//!   up a run each. A manual refresh when idle simply runs.
//!
//! The timer is a gap between the end of one run and the start of the next, not a fixed rate, so a
//! slow run can never be followed by a pile of due ticks. Time comes from a [`Clock`] so the
//! schedule is tested without waiting.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

/// The gap between two scheduled runs.
pub(super) const INTERVAL: Duration = Duration::from_secs(10 * 60);

/// What asked for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Startup,
    Timer,
    Manual,
}

/// Lets at most one run be in progress and decides what a request made during a run does.
pub(super) struct SingleFlight<T> {
    /// Held for the length of a run; the result of the last finished run is kept inside.
    run: Mutex<Option<T>>,
    /// Runs started so far. A manual request compares it before and after it waited.
    started: AtomicU64,
}

impl<T: Clone> SingleFlight<T> {
    pub(super) fn new() -> Self {
        Self {
            run: Mutex::new(None),
            started: AtomicU64::new(0),
        }
    }

    /// Runs `work` unless a run is in progress, in which case it returns `None` at once.
    pub(super) async fn run_unless_busy<F, Fut>(&self, work: F) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let mut last = self.run.try_lock().ok()?;
        self.started.fetch_add(1, Ordering::SeqCst);
        let result = work().await;
        *last = Some(result.clone());
        Some(result)
    }

    /// Runs `work` with no run in progress and none able to start until it ends: it waits for a
    /// run that is going, and a start or timer run that comes meanwhile does nothing. For cleanup
    /// (disable, logout), which must not interleave with a run that creates placeholders.
    pub(super) async fn exclusive<R, F, Fut>(&self, work: F) -> R
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = R>,
    {
        let _held = self.run.lock().await;
        work().await
    }

    /// Runs `work` after any run in progress has finished, unless another run began after this
    /// request in the meantime, in which case that run's result is returned instead.
    pub(super) async fn run_after_current<F, Fut>(&self, work: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let seen = self.started.load(Ordering::SeqCst);
        let mut last = self.run.lock().await;
        if self.started.load(Ordering::SeqCst) != seen {
            if let Some(result) = last.clone() {
                return result;
            }
        }
        self.started.fetch_add(1, Ordering::SeqCst);
        let result = work().await;
        *last = Some(result.clone());
        result
    }
}

/// Time, as the schedule needs it.
pub(super) trait Clock: Sync {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// The real clock.
pub(super) struct TokioClock;

impl Clock for TokioClock {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Calls `run` for the start, then once per `interval` after the previous call returned, for as
/// long as the future is polled.
pub(super) async fn run_schedule<C, F, Fut>(clock: &C, interval: Duration, mut run: F)
where
    C: Clock,
    F: FnMut(Trigger) -> Fut,
    Fut: Future<Output = ()>,
{
    run(Trigger::Startup).await;
    loop {
        clock.sleep(interval).await;
        run(Trigger::Timer).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::{mpsc, oneshot, Notify};

    /// A run the test controls: it announces that it started and finishes when told to.
    struct Gate {
        started: Arc<Notify>,
        release: StdMutex<Option<oneshot::Receiver<u32>>>,
    }

    fn gate() -> (Gate, oneshot::Sender<u32>, Arc<Notify>) {
        let (send, receive) = oneshot::channel();
        let started = Arc::new(Notify::new());
        (
            Gate {
                started: started.clone(),
                release: StdMutex::new(Some(receive)),
            },
            send,
            started,
        )
    }

    impl Gate {
        async fn run(&self) -> u32 {
            let Some(receive) = self.release.lock().unwrap().take() else {
                return 0;
            };
            self.started.notify_one();
            receive.await.unwrap()
        }
    }

    fn counted(counter: &Arc<StdMutex<u32>>) -> impl Future<Output = u32> {
        let counter = counter.clone();
        async move {
            let mut count = counter.lock().unwrap();
            *count += 1;
            *count
        }
    }

    #[tokio::test]
    async fn a_background_run_when_idle_runs() {
        let flight = SingleFlight::new();
        let counter = Arc::new(StdMutex::new(0));

        let result = flight.run_unless_busy(|| counted(&counter)).await;

        assert_eq!(result, Some(1));
    }

    #[tokio::test]
    async fn a_start_or_timer_run_during_a_run_does_nothing() {
        let flight = Arc::new(SingleFlight::new());
        let (running, release, started) = gate();
        let first = tokio::spawn({
            let flight = flight.clone();
            async move { flight.run_unless_busy(|| running.run()).await }
        });
        started.notified().await;
        let counter = Arc::new(StdMutex::new(0));

        let overlapping = flight.run_unless_busy(|| counted(&counter)).await;
        release.send(7).unwrap();

        assert_eq!(overlapping, None);
        assert_eq!(*counter.lock().unwrap(), 0, "the work never ran");
        assert_eq!(first.await.unwrap(), Some(7));
    }

    #[tokio::test]
    async fn a_manual_refresh_when_idle_runs_at_once() {
        let flight = SingleFlight::new();
        let counter = Arc::new(StdMutex::new(0));

        assert_eq!(flight.run_after_current(|| counted(&counter)).await, 1);
        assert_eq!(flight.run_after_current(|| counted(&counter)).await, 2);
    }

    #[tokio::test]
    async fn a_manual_refresh_during_a_run_waits_for_it_and_then_runs_once_more() {
        let flight = Arc::new(SingleFlight::new());
        let (running, release, started) = gate();
        let first = tokio::spawn({
            let flight = flight.clone();
            async move { flight.run_unless_busy(|| running.run()).await }
        });
        started.notified().await;
        let counter = Arc::new(StdMutex::new(0));
        let manual = tokio::spawn({
            let flight = flight.clone();
            let counter = counter.clone();
            async move { flight.run_after_current(|| counted(&counter)).await }
        });
        tokio::task::yield_now().await;
        assert_eq!(*counter.lock().unwrap(), 0, "waits while the first runs");

        release.send(7).unwrap();

        assert_eq!(first.await.unwrap(), Some(7));
        assert_eq!(manual.await.unwrap(), 1, "a fresh run, not the first one's");
    }

    #[tokio::test]
    async fn manual_refreshes_during_one_run_share_a_single_follow_up_run() {
        let flight = Arc::new(SingleFlight::new());
        let (running, release, started) = gate();
        let first = tokio::spawn({
            let flight = flight.clone();
            async move { flight.run_unless_busy(|| running.run()).await }
        });
        started.notified().await;
        let counter = Arc::new(StdMutex::new(0));
        let manuals = (0..3)
            .map(|_| {
                let flight = flight.clone();
                let counter = counter.clone();
                tokio::spawn(async move { flight.run_after_current(|| counted(&counter)).await })
            })
            .collect::<Vec<_>>();
        tokio::task::yield_now().await;

        release.send(7).unwrap();
        first.await.unwrap();
        let mut results = Vec::new();
        for manual in manuals {
            results.push(manual.await.unwrap());
        }

        assert_eq!(results, vec![1, 1, 1]);
        assert_eq!(*counter.lock().unwrap(), 1, "one run for all three");
    }

    #[tokio::test]
    async fn an_exclusive_section_waits_for_a_run_in_progress() {
        let flight = Arc::new(SingleFlight::new());
        let (running, release, started) = gate();
        let first = tokio::spawn({
            let flight = flight.clone();
            async move { flight.run_unless_busy(|| running.run()).await }
        });
        started.notified().await;
        let counter = Arc::new(StdMutex::new(0));
        let exclusive = tokio::spawn({
            let flight = flight.clone();
            let counter = counter.clone();
            async move { flight.exclusive(|| counted(&counter)).await }
        });
        tokio::task::yield_now().await;
        assert_eq!(*counter.lock().unwrap(), 0, "cleanup waits for the run");

        release.send(7).unwrap();

        assert_eq!(first.await.unwrap(), Some(7));
        assert_eq!(exclusive.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn no_run_starts_while_an_exclusive_section_is_in_progress() {
        let flight = Arc::new(SingleFlight::<u32>::new());
        let (held, release, started) = gate();
        let exclusive = tokio::spawn({
            let flight = flight.clone();
            async move { flight.exclusive(|| held.run()).await }
        });
        started.notified().await;
        let counter = Arc::new(StdMutex::new(0));

        let timer = flight.run_unless_busy(|| counted(&counter)).await;
        let manual = tokio::spawn({
            let flight = flight.clone();
            let counter = counter.clone();
            async move { flight.run_after_current(|| counted(&counter)).await }
        });
        tokio::task::yield_now().await;
        assert_eq!(timer, None);
        assert_eq!(
            *counter.lock().unwrap(),
            0,
            "nothing ran during the section"
        );

        release.send(1).unwrap();
        exclusive.await.unwrap();

        assert_eq!(
            manual.await.unwrap(),
            1,
            "the manual refresh ran afterwards"
        );
    }

    #[tokio::test]
    async fn the_next_background_run_is_allowed_once_the_run_has_finished() {
        let flight = SingleFlight::new();
        let counter = Arc::new(StdMutex::new(0));
        flight.run_unless_busy(|| counted(&counter)).await;

        assert_eq!(flight.run_unless_busy(|| counted(&counter)).await, Some(2));
    }

    /// A clock the test advances by hand: each `sleep` waits for one tick from the test and
    /// records how long it was asked to sleep.
    struct FakeClock {
        ticks: tokio::sync::Mutex<mpsc::UnboundedReceiver<()>>,
        sleeps: StdMutex<Vec<Duration>>,
    }

    impl Clock for FakeClock {
        async fn sleep(&self, duration: Duration) {
            self.sleeps.lock().unwrap().push(duration);
            self.ticks.lock().await.recv().await;
        }
    }

    #[tokio::test]
    async fn the_schedule_runs_at_start_and_then_after_every_interval() {
        let (advance, ticks) = mpsc::unbounded_channel();
        let clock = Arc::new(FakeClock {
            ticks: tokio::sync::Mutex::new(ticks),
            sleeps: StdMutex::new(Vec::new()),
        });
        let runs = Arc::new(StdMutex::new(Vec::new()));
        let (progress, mut progressed) = mpsc::unbounded_channel();
        let schedule = tokio::spawn({
            let clock = clock.clone();
            let runs = runs.clone();
            async move {
                run_schedule(&*clock, INTERVAL, |trigger| {
                    let runs = runs.clone();
                    let progress = progress.clone();
                    async move {
                        runs.lock().unwrap().push(trigger);
                        let _ = progress.send(());
                    }
                })
                .await;
            }
        });

        progressed.recv().await.unwrap();
        assert_eq!(*runs.lock().unwrap(), vec![Trigger::Startup]);
        advance.send(()).unwrap();
        progressed.recv().await.unwrap();
        advance.send(()).unwrap();
        progressed.recv().await.unwrap();
        schedule.abort();

        assert_eq!(
            *runs.lock().unwrap(),
            vec![Trigger::Startup, Trigger::Timer, Trigger::Timer]
        );
        assert_eq!(
            *clock.sleeps.lock().unwrap(),
            vec![INTERVAL, INTERVAL, INTERVAL]
        );
    }

    #[test]
    fn the_interval_is_ten_minutes() {
        assert_eq!(INTERVAL, Duration::from_secs(600));
    }

    #[tokio::test]
    async fn a_slow_run_delays_the_next_tick_instead_of_stacking_ticks() {
        let (_advance, ticks) = mpsc::unbounded_channel();
        let clock = Arc::new(FakeClock {
            ticks: tokio::sync::Mutex::new(ticks),
            sleeps: StdMutex::new(Vec::new()),
        });
        let (running, release, started) = gate();
        let running = Arc::new(running);
        let finished = Arc::new(Notify::new());
        let schedule = tokio::spawn({
            let clock = clock.clone();
            let finished = finished.clone();
            async move {
                run_schedule(&*clock, INTERVAL, |_| {
                    let running = running.clone();
                    let finished = finished.clone();
                    async move {
                        running.run().await;
                        finished.notify_one();
                    }
                })
                .await;
            }
        });
        started.notified().await;
        // While the start run is still going the clock has not even been asked to sleep, so no tick
        // can be due.
        tokio::task::yield_now().await;
        assert!(clock.sleeps.lock().unwrap().is_empty());

        release.send(0).unwrap();
        finished.notified().await;
        tokio::task::yield_now().await;
        schedule.abort();

        assert_eq!(clock.sleeps.lock().unwrap().len(), 1);
    }
}
