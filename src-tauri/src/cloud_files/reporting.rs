//! Telling the user about a failed request, once per failure episode (ADR-0003, #281).
//!
//! Explorer, a thumbnail pass or a backup tool can open hundreds of placeholders in a row while
//! Immich is unreachable. Every one of them fails promptly with its own Cloud Files error, but the
//! user gets one in-app notice, not hundreds.
//!
//! The episode rule, per kind of failure ([`NoticeKind`]):
//! - the first failure of a kind starts an episode and produces the notice;
//! - further failures of the same kind inside the episode produce nothing, and each one keeps the
//!   episode going: it ends only after [`QUIET_PERIOD`] without a failure of that kind;
//! - a success (any request Immich answered) ends the `Offline`, `Unauthorized` and `Failed`
//!   episodes at once, because the cause is gone: the next failure is news again;
//! - a `NotFound` episode is not ended by a success elsewhere, since one removed photo says
//!   nothing about the others; it ends only by the quiet period.
//!
//! Different kinds are independent episodes.
//!
//! The rule is [`Episodes`], pure and driven by a [`Clock`] so tests use a fake one. [`Reporter`]
//! adds the destination ([`NoticeSink`], the app) and the process-wide instance the Windows
//! callbacks report to.

use super::failure::{Failure, FailureKind};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a kind of failure must stay away before the next one is a new episode.
pub(super) const QUIET_PERIOD: Duration = Duration::from_secs(5 * 60);

/// What the user is told, in the terms of what they can do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NoticeKind {
    /// Immich could not be reached or did not answer properly.
    Offline,
    /// The sign-in is no longer accepted; the frontend offers to sign in again.
    Unauthorized,
    /// The photo is gone from Immich. The placeholder disappears at the next reconciliation (#280).
    NotFound,
    /// Anything else the user can be told about.
    Failed,
}

impl NoticeKind {
    /// `None` for failures that need no notice.
    pub(super) fn of(kind: FailureKind) -> Option<Self> {
        match kind {
            FailureKind::Offline => Some(Self::Offline),
            FailureKind::Unauthorized => Some(Self::Unauthorized),
            FailureKind::NotFound => Some(Self::NotFound),
            FailureKind::Failed => Some(Self::Failed),
            FailureKind::Platform => None,
        }
    }

    /// The code the frontend knows the kind by; also the suffix of the log and locale keys
    /// (`log_virtual_library_<code>`).
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::Failed => "failed",
        }
    }
}

/// One notice for the user. Serialised as the payload of the `virtual-library-notice` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Notice {
    pub kind: NoticeKind,
    /// The photo or folder the failed request was about.
    pub name: String,
}

impl Serialize for NoticeKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl Notice {
    /// The English text, which the log entry carries for readers without the locale key.
    pub(crate) fn message(&self) -> String {
        let name = &self.name;
        match self.kind {
            NoticeKind::Offline => format!(
                "Could not open '{name}' from the Virtual Library because Immich could not be reached."
            ),
            NoticeKind::Unauthorized => format!(
                "Could not open '{name}' from the Virtual Library because your Immich sign-in is no longer valid. Sign in again to continue."
            ),
            NoticeKind::NotFound => format!(
                "Could not open '{name}' from the Virtual Library because it was removed from Immich. It disappears at the next refresh."
            ),
            NoticeKind::Failed => {
                format!("Could not open '{name}' from the Virtual Library.")
            }
        }
    }
}

/// What the failed request was doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    /// A placeholder was opened (hydration).
    Open,
    /// A directory was listed for the first time.
    List,
}

/// A failed request, as the adapter reports it.
pub(super) struct Report<'a> {
    pub operation: Operation,
    /// The path in the Virtual Library, relative to its root.
    pub path: &'a str,
    pub failure: &'a Failure,
}

impl Report<'_> {
    /// The path of a placeholder whose Remote Asset has been removed from Immich: the hook for
    /// #280, whose reconciliation owns removing placeholders. Today it is only reported to the
    /// reader and the user; reconciliation finds the same asset gone from its listing.
    pub(super) fn removed_asset_path(&self) -> Option<&str> {
        (self.operation == Operation::Open && self.failure.kind == FailureKind::NotFound)
            .then_some(self.path)
    }

    /// The last path component, which is what the user knows the item by.
    fn name(&self) -> String {
        self.path
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or("")
            .to_string()
    }
}

/// Monotonic time, injectable.
pub(super) trait Clock: Send {
    /// Time since an arbitrary fixed origin.
    fn now(&self) -> Duration;
}

pub(super) struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub(super) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// The episode rule (see the module documentation).
pub(super) struct Episodes<C: Clock> {
    clock: C,
    quiet: Duration,
    /// The time of the latest failure of each kind that has an open episode.
    latest: HashMap<NoticeKind, Duration>,
}

impl<C: Clock> Episodes<C> {
    pub(super) fn new(clock: C, quiet: Duration) -> Self {
        Self {
            clock,
            quiet,
            latest: HashMap::new(),
        }
    }

    /// Records a failure of `kind`; true when it starts an episode, which is when the user is told.
    pub(super) fn failed(&mut self, kind: NoticeKind) -> bool {
        let now = self.clock.now();
        let continues = self
            .latest
            .get(&kind)
            .is_some_and(|latest| now.saturating_sub(*latest) < self.quiet);
        self.latest.insert(kind, now);
        !continues
    }

    /// Records a request Immich answered: whatever was wrong with the server or the sign-in is not
    /// any more.
    pub(super) fn succeeded(&mut self) {
        self.latest.retain(|kind, _| *kind == NoticeKind::NotFound);
    }
}

/// Where a notice goes: in the app, the activity log and the event the frontend shows.
pub(crate) trait NoticeSink: Send + Sync {
    fn show(&self, notice: &Notice);
}

/// Decides which failures become notices and hands those to the sink.
pub(super) struct Reporter<C: Clock> {
    episodes: Mutex<Episodes<C>>,
    sink: Box<dyn NoticeSink>,
}

impl<C: Clock> Reporter<C> {
    pub(super) fn new(clock: C, sink: Box<dyn NoticeSink>) -> Self {
        Self {
            episodes: Mutex::new(Episodes::new(clock, QUIET_PERIOD)),
            sink,
        }
    }

    /// Reports a failed request. Returns the notice shown, if it started an episode.
    pub(super) fn failed(&self, report: &Report<'_>) -> Option<Notice> {
        let kind = NoticeKind::of(report.failure.kind)?;
        let starts_episode = self.episodes.lock().ok()?.failed(kind);
        if !starts_episode {
            return None;
        }
        let notice = Notice {
            kind,
            name: report.name(),
        };
        self.sink.show(&notice);
        Some(notice)
    }

    pub(super) fn succeeded(&self) {
        if let Ok(mut episodes) = self.episodes.lock() {
            episodes.succeeded();
        }
    }
}

static REPORTER: OnceLock<Reporter<SystemClock>> = OnceLock::new();

/// Sets where the Windows callbacks' notices go. The first call wins; the app calls it once at
/// start.
pub(crate) fn install(sink: Box<dyn NoticeSink>) {
    let _ = REPORTER.set(Reporter::new(SystemClock::new(), sink));
}

/// Reports a failed request to the user, once per episode. Without a sink (not yet installed) the
/// failure is only the reader's error.
pub(super) fn report_failed(report: &Report<'_>) {
    if let Some(reporter) = REPORTER.get() {
        reporter.failed(report);
    }
}

/// Reports that Immich answered a request.
pub(super) fn report_succeeded() {
    if let Some(reporter) = REPORTER.get() {
        reporter.succeeded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeClock(Arc<Mutex<Duration>>);

    impl FakeClock {
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.0.lock().unwrap()
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<Notice>>>);

    impl NoticeSink for RecordingSink {
        fn show(&self, notice: &Notice) {
            self.0.lock().unwrap().push(notice.clone());
        }
    }

    const MINUTE: Duration = Duration::from_secs(60);

    fn reporter() -> (Reporter<FakeClock>, FakeClock, RecordingSink) {
        let clock = FakeClock::default();
        let sink = RecordingSink::default();
        (
            Reporter::new(clock.clone(), Box::new(sink.clone())),
            clock,
            sink,
        )
    }

    fn fail(reporter: &Reporter<FakeClock>, operation: Operation, kind: FailureKind, path: &str) {
        let failure = Failure::new(kind, "x");
        reporter.failed(&Report {
            operation,
            path,
            failure: &failure,
        });
    }

    fn open(reporter: &Reporter<FakeClock>, kind: FailureKind) {
        fail(
            reporter,
            Operation::Open,
            kind,
            "All Photos/2025/03 March/a.jpg",
        );
    }

    fn shown(sink: &RecordingSink) -> Vec<NoticeKind> {
        sink.0.lock().unwrap().iter().map(|n| n.kind).collect()
    }

    #[test]
    fn a_burst_of_failures_of_one_kind_is_one_notice() {
        let (reporter, _clock, sink) = reporter();

        for _ in 0..50 {
            open(&reporter, FailureKind::Offline);
        }

        assert_eq!(shown(&sink), [NoticeKind::Offline]);
    }

    #[test]
    fn failures_that_keep_coming_keep_the_episode_going() {
        let (reporter, clock, sink) = reporter();

        // Slower than a burst, but never a quiet period apart: still one episode.
        for _ in 0..10 {
            open(&reporter, FailureKind::Offline);
            clock.advance(QUIET_PERIOD - MINUTE);
        }

        assert_eq!(shown(&sink), [NoticeKind::Offline]);
    }

    #[test]
    fn a_quiet_period_ends_the_episode_so_the_next_failure_is_news_again() {
        let (reporter, clock, sink) = reporter();

        open(&reporter, FailureKind::Offline);
        clock.advance(QUIET_PERIOD);
        open(&reporter, FailureKind::Offline);

        assert_eq!(shown(&sink), [NoticeKind::Offline, NoticeKind::Offline]);
    }

    #[test]
    fn a_success_ends_a_server_episode_at_once() {
        let (reporter, _clock, sink) = reporter();

        open(&reporter, FailureKind::Offline);
        open(&reporter, FailureKind::Unauthorized);
        reporter.succeeded();
        open(&reporter, FailureKind::Offline);
        open(&reporter, FailureKind::Unauthorized);

        assert_eq!(
            shown(&sink),
            [
                NoticeKind::Offline,
                NoticeKind::Unauthorized,
                NoticeKind::Offline,
                NoticeKind::Unauthorized
            ]
        );
    }

    #[test]
    fn a_success_elsewhere_does_not_end_a_removed_photo_episode() {
        let (reporter, clock, sink) = reporter();

        open(&reporter, FailureKind::NotFound);
        reporter.succeeded();
        open(&reporter, FailureKind::NotFound);
        assert_eq!(shown(&sink), [NoticeKind::NotFound]);

        clock.advance(QUIET_PERIOD);
        open(&reporter, FailureKind::NotFound);
        assert_eq!(shown(&sink).len(), 2);
    }

    #[test]
    fn different_kinds_are_separate_episodes() {
        let (reporter, _clock, sink) = reporter();

        open(&reporter, FailureKind::Offline);
        open(&reporter, FailureKind::Unauthorized);
        open(&reporter, FailureKind::Offline);

        assert_eq!(
            shown(&sink),
            [NoticeKind::Offline, NoticeKind::Unauthorized]
        );
    }

    #[test]
    fn a_failure_the_platform_caused_is_never_a_notice() {
        let (reporter, _clock, sink) = reporter();

        open(&reporter, FailureKind::Platform);

        assert!(shown(&sink).is_empty());
    }

    #[test]
    fn the_notice_names_the_item_by_its_last_path_component() {
        let (reporter, _clock, sink) = reporter();

        open(&reporter, FailureKind::Offline);
        fail(
            &reporter,
            Operation::List,
            FailureKind::Unauthorized,
            "Albums/Holiday",
        );
        fail(&reporter, Operation::List, FailureKind::NotFound, "");

        let names: Vec<String> = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|n| n.name.clone())
            .collect();
        assert_eq!(names, ["a.jpg", "Holiday", ""]);
    }

    #[test]
    fn the_event_payload_carries_the_kind_code_and_the_name() {
        let notice = Notice {
            kind: NoticeKind::NotFound,
            name: "a.jpg".into(),
        };

        assert_eq!(
            serde_json::to_value(&notice).unwrap(),
            serde_json::json!({ "kind": "not_found", "name": "a.jpg" })
        );
    }

    #[test]
    fn an_opened_placeholder_whose_asset_is_gone_is_a_removed_asset_for_reconciliation() {
        let gone = Failure::new(FailureKind::NotFound, "x");
        let offline = Failure::new(FailureKind::Offline, "x");
        let report = |operation, failure| {
            Report {
                operation,
                path: "All Photos/2025/03 March/a.jpg",
                failure,
            }
            .removed_asset_path()
            .map(str::to_string)
        };

        assert_eq!(
            report(Operation::Open, &gone).as_deref(),
            Some("All Photos/2025/03 March/a.jpg")
        );
        assert_eq!(report(Operation::Open, &offline), None);
        assert_eq!(report(Operation::List, &gone), None);
    }
}
