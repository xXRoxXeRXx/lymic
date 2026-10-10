//! Shared byte progress and runtime transfer statistics across queue blocks.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::Emitter;

/// Duration of the recent-transfer window used when calculating the upload rate.
pub(crate) const TRANSFER_RATE_WINDOW: Duration = Duration::from_secs(3);
/// Minimum interval between eligible transfer-statistics updates.
pub(crate) const TRANSFER_STATS_THROTTLE: Duration = Duration::from_millis(250);

/// A serializable view of the transfer metrics suitable for UI events.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferStatsSnapshot {
    pub(crate) transferred_bytes: u64,
    pub(crate) transfer_rate_bytes_per_second: f64,
    pub(crate) estimated_seconds_remaining: Option<u64>,
}

/// Complete payload emitted to native and web UI consumers.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferStatsEvent {
    #[serde(flatten)]
    pub(crate) stats: TransferStatsSnapshot,
    pub(crate) status: &'static str,
    pub(crate) total_bytes: u64,
    pub(crate) completed_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TransferStatus {
    pub(crate) active: bool,
    pub(crate) paused: bool,
}

#[derive(Debug)]
struct TransferStatsState {
    total_bytes: u64,
    completed_bytes: u64,
    transferred_bytes: u64,
    samples: VecDeque<(Instant, u64)>,
    frozen_rate: Option<f64>,
    status: TransferStatus,
    last_emission: Option<Instant>,
}

/// Thread-safe runtime transfer accounting shared by concurrent upload workers.
#[derive(Clone, Debug)]
pub(crate) struct TransferStatsTracker {
    state: Arc<Mutex<TransferStatsState>>,
}

impl TransferStatsTracker {
    pub(crate) fn new(total_bytes: u64, completed_bytes: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(TransferStatsState {
                total_bytes,
                completed_bytes: completed_bytes.min(total_bytes),
                transferred_bytes: 0,
                samples: VecDeque::new(),
                frozen_rate: None,
                status: TransferStatus::default(),
                last_emission: None,
            })),
        }
    }

    /// Reconciles durable queue totals without treating restored bytes as new transfer data.
    pub(crate) fn sync_queue(&self, total_bytes: u64, completed_bytes: u64) {
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        state.total_bytes = total_bytes;
        state.completed_bytes = completed_bytes.min(total_bytes);
    }

    /// Records bytes transferred by an active worker.
    pub(crate) fn record_chunk(&self, bytes: u64) {
        self.record_chunk_at(bytes, Instant::now());
    }

    /// Marks whether the queue currently has active transfer work.
    pub(crate) fn set_active(&self, active: bool) {
        self.state
            .lock()
            .expect("transfer statistics lock poisoned")
            .status
            .active = active;
    }

    /// Marks whether transfer work is currently paused.
    pub(crate) fn set_paused(&self, paused: bool) {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        state.status.paused = paused;
        state.frozen_rate = paused.then(|| calculate_rate(&state.samples, now));
    }

    pub(crate) fn status(&self) -> TransferStatus {
        self.state
            .lock()
            .expect("transfer statistics lock poisoned")
            .status
    }

    /// Returns whether an event may be emitted now, advancing the throttle when it may.
    pub(crate) fn should_emit(&self) -> bool {
        self.should_emit_at(Instant::now())
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> TransferStatsSnapshot {
        self.snapshot_at(Instant::now())
    }

    fn record_chunk_at(&self, bytes: u64, now: Instant) {
        if bytes == 0 {
            return;
        }

        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        state.transferred_bytes = state.transferred_bytes.saturating_add(bytes);
        state.samples.push_back((now, bytes));
        prune_samples(&mut state.samples, now);
    }

    #[cfg(test)]
    fn snapshot_at(&self, now: Instant) -> TransferStatsSnapshot {
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        snapshot_at_locked(&mut state, now)
    }

    pub(crate) fn advance_completed(&self, bytes: u64) {
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        state.completed_bytes = state
            .completed_bytes
            .saturating_add(bytes)
            .min(state.total_bytes);
    }

    pub(crate) fn event(&self) -> TransferStatsEvent {
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        let status = if state.status.paused {
            "PAUSED"
        } else if state.status.active {
            "RUNNING"
        } else {
            "IDLE"
        };
        TransferStatsEvent {
            stats: snapshot_at_locked(&mut state, Instant::now()),
            status,
            total_bytes: state.total_bytes,
            completed_bytes: state.completed_bytes,
        }
    }

    fn should_emit_at(&self, now: Instant) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("transfer statistics lock poisoned");
        let eligible = state
            .last_emission
            .is_none_or(|last| now.duration_since(last) >= TRANSFER_STATS_THROTTLE);
        if eligible {
            state.last_emission = Some(now);
        }
        eligible
    }
}

fn prune_samples(samples: &mut VecDeque<(Instant, u64)>, now: Instant) {
    while samples
        .front()
        .is_some_and(|(recorded_at, _)| now.duration_since(*recorded_at) > TRANSFER_RATE_WINDOW)
    {
        samples.pop_front();
    }
}

fn calculate_rate(samples: &VecDeque<(Instant, u64)>, now: Instant) -> f64 {
    let Some((first_at, _)) = samples.front() else {
        return 0.0;
    };
    let elapsed = now.duration_since(*first_at).as_secs_f64();
    if elapsed == 0.0 {
        return 0.0;
    }

    samples.iter().map(|(_, bytes)| *bytes as f64).sum::<f64>() / elapsed
}

fn snapshot_at_locked(state: &mut TransferStatsState, now: Instant) -> TransferStatsSnapshot {
    prune_samples(&mut state.samples, now);
    let rate = if !state.status.active {
        0.0
    } else {
        state
            .frozen_rate
            .unwrap_or_else(|| calculate_rate(&state.samples, now))
    };
    let remaining = state.total_bytes.saturating_sub(state.completed_bytes);
    let estimated_seconds_remaining = if !state.status.active || state.status.paused {
        None
    } else if remaining == 0 {
        Some(0)
    } else if rate > 0.0 {
        Some((remaining as f64 / rate).ceil() as u64)
    } else {
        None
    };
    TransferStatsSnapshot {
        transferred_bytes: state.transferred_bytes,
        transfer_rate_bytes_per_second: rate,
        estimated_seconds_remaining,
    }
}

/// Calculates the progress percentage rounded to the nearest integer.
/// Returns 0% when total is 0 (matching the frontend idle/empty queue state).
pub(crate) fn calculate_percent(done: u64, total: u64) -> u32 {
    if total == 0 {
        0
    } else {
        ((done as f64 / total as f64 * 100.0).round() as u32).min(100)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JobByteProgress {
    total_bytes: Arc<AtomicU64>,
    completed_bytes: Arc<AtomicU64>,
    transfer_stats: TransferStatsTracker,
}

impl JobByteProgress {
    pub(crate) fn new(total_bytes: u64, completed_bytes: u64) -> Self {
        Self {
            total_bytes: Arc::new(AtomicU64::new(total_bytes)),
            completed_bytes: Arc::new(AtomicU64::new(completed_bytes)),
            transfer_stats: TransferStatsTracker::new(total_bytes, completed_bytes),
        }
    }

    pub(crate) fn sync_from_snapshot(&self, total_bytes: u64, completed_bytes: u64) {
        self.total_bytes.store(total_bytes, Ordering::SeqCst);
        self.completed_bytes
            .store(completed_bytes, Ordering::SeqCst);
        self.transfer_stats.sync_queue(total_bytes, completed_bytes);
    }

    pub(crate) fn add_bytes(&self, bytes: u64) -> (u64, u64, u32) {
        let done = self.completed_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes;
        let total = self.total_bytes.load(Ordering::SeqCst);
        self.transfer_stats.advance_completed(bytes);
        (done, total, calculate_percent(done, total))
    }

    pub(crate) fn transfer_stats(&self) -> TransferStatsTracker {
        self.transfer_stats.clone()
    }

    pub(crate) fn advance(&self, app: &tauri::AppHandle, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let (_, _, pct) = self.add_bytes(bytes);
        let _ = app.emit("sync-progress-percent", pct);
    }

    #[cfg(test)]
    pub(crate) fn current_percent(&self) -> u32 {
        let total = self.total_bytes.load(Ordering::SeqCst);
        let done = self.completed_bytes.load(Ordering::SeqCst);
        calculate_percent(done, total)
    }

    #[cfg(test)]
    pub(crate) fn completed_bytes(&self) -> u64 {
        self.completed_bytes.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_percent_matches_rounding_and_bounds() {
        assert_eq!(calculate_percent(0, 0), 0);
        assert_eq!(calculate_percent(0, 10_000), 0);
        assert_eq!(calculate_percent(2_500, 10_000), 25);
        assert_eq!(calculate_percent(1, 200), 1);
        assert_eq!(calculate_percent(15_000, 10_000), 100);
    }

    #[test]
    fn calculates_percentage_correctly_across_blocks() {
        let progress = JobByteProgress::new(10_000, 0);
        progress.add_bytes(2_500);
        assert_eq!(progress.current_percent(), 25);
        progress.add_bytes(2_500);
        assert_eq!(progress.current_percent(), 50);
        progress.add_bytes(5_000);
        assert_eq!(progress.current_percent(), 100);
    }

    #[test]
    fn handles_zero_total_gracefully() {
        let progress = JobByteProgress::new(0, 0);
        assert_eq!(progress.current_percent(), 0);
    }

    #[test]
    fn sync_from_snapshot_reconciles_total_and_completed_bytes() {
        let progress = JobByteProgress::new(5_000, 2_000);
        progress.add_bytes(1_000);
        progress.sync_from_snapshot(10_000, 3_000);
        assert_eq!(progress.total_bytes(), 10_000);
        assert_eq!(progress.completed_bytes(), 3_000);
        assert_eq!(progress.current_percent(), 30);
    }

    #[test]
    fn calculates_rate_from_recent_chunks() {
        let tracker = TransferStatsTracker::new(10_000, 0);
        tracker.set_active(true);
        let start = Instant::now();
        tracker.record_chunk_at(500, start);
        tracker.record_chunk_at(500, start + Duration::from_secs(1));
        let snapshot = tracker.snapshot_at(start + Duration::from_secs(1));

        assert_eq!(snapshot.transferred_bytes, 1_000);
        assert_eq!(snapshot.transfer_rate_bytes_per_second, 1_000.0);
        assert_eq!(snapshot.estimated_seconds_remaining, Some(10));
    }

    #[test]
    fn returns_zero_eta_when_no_bytes_remain() {
        let tracker = TransferStatsTracker::new(1_000, 1_000);
        tracker.set_active(true);
        let snapshot = tracker.snapshot();

        assert_eq!(snapshot.estimated_seconds_remaining, Some(0));
        assert_eq!(snapshot.transfer_rate_bytes_per_second, 0.0);
    }

    #[test]
    fn clamps_completed_bytes_and_remaining_work_to_queue_total() {
        let tracker = TransferStatsTracker::new(1_000, 2_000);
        tracker.set_active(true);
        let start = Instant::now();
        tracker.record_chunk_at(100, start);
        let snapshot = tracker.snapshot_at(start + Duration::from_secs(1));

        assert_eq!(snapshot.transferred_bytes, 100);
        assert_eq!(snapshot.estimated_seconds_remaining, Some(0));
    }

    #[test]
    fn evicts_chunks_outside_the_rolling_window() {
        let tracker = TransferStatsTracker::new(10_000, 0);
        let start = Instant::now();
        tracker.record_chunk_at(500, start);
        tracker.record_chunk_at(500, start + TRANSFER_RATE_WINDOW + Duration::from_millis(1));
        let snapshot =
            tracker.snapshot_at(start + (TRANSFER_RATE_WINDOW * 2) + Duration::from_millis(2));

        assert_eq!(snapshot.transfer_rate_bytes_per_second, 0.0);
        assert_eq!(snapshot.estimated_seconds_remaining, None);
    }

    #[test]
    fn throttle_allows_the_first_update_then_enforces_its_interval() {
        let tracker = TransferStatsTracker::new(0, 0);
        let start = Instant::now();

        assert!(tracker.should_emit_at(start));
        assert!(!tracker.should_emit_at(start + TRANSFER_STATS_THROTTLE - Duration::from_millis(1)));
        assert!(tracker.should_emit_at(start + TRANSFER_STATS_THROTTLE));
    }

    #[test]
    fn tracks_active_and_paused_status() {
        let tracker = TransferStatsTracker::new(0, 0);
        tracker.set_active(true);
        tracker.set_paused(true);

        assert_eq!(
            tracker.status(),
            TransferStatus {
                active: true,
                paused: true,
            }
        );
    }

    #[test]
    fn snapshot_uses_camel_case_serialization() {
        let snapshot = TransferStatsTracker::new(0, 0).snapshot();
        let value = serde_json::to_value(snapshot).unwrap();

        assert!(value.get("transferredBytes").is_some());
        assert!(value.get("transferRateBytesPerSecond").is_some());
        assert!(value.get("estimatedSecondsRemaining").is_some());
    }
}
