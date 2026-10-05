//! Shared byte progress tracking across queue blocks.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tauri::Emitter;

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
}

impl JobByteProgress {
    pub(crate) fn new(total_bytes: u64, completed_bytes: u64) -> Self {
        Self {
            total_bytes: Arc::new(AtomicU64::new(total_bytes)),
            completed_bytes: Arc::new(AtomicU64::new(completed_bytes)),
        }
    }

    pub(crate) fn sync_from_snapshot(&self, total_bytes: u64, completed_bytes: u64) {
        self.total_bytes.store(total_bytes, Ordering::SeqCst);
        self.completed_bytes
            .store(completed_bytes, Ordering::SeqCst);
    }

    pub(crate) fn add_bytes(&self, bytes: u64) -> (u64, u64, u32) {
        let done = self.completed_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes;
        let total = self.total_bytes.load(Ordering::SeqCst);
        (done, total, calculate_percent(done, total))
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
        assert_eq!(calculate_percent(1, 200), 1); // 0.5% rounds to 1%
        assert_eq!(calculate_percent(15_000, 10_000), 100);
    }

    #[test]
    fn handles_zero_total_gracefully() {
        let progress = JobByteProgress::new(0, 0);
        assert_eq!(progress.current_percent(), 0);
    }

    #[test]
    fn calculates_percentage_correctly_across_blocks() {
        let progress = JobByteProgress::new(10_000, 0);
        assert_eq!(progress.current_percent(), 0);

        // Advance by 2,500 bytes (first block partial)
        let (_, _, pct) = progress.add_bytes(2_500);
        assert_eq!(pct, 25);
        assert_eq!(progress.current_percent(), 25);

        // Advance by another 2,500 bytes (end of first block, 50%)
        let (_, _, pct) = progress.add_bytes(2_500);
        assert_eq!(pct, 50);
        assert_eq!(progress.current_percent(), 50);

        // Second block starts and advances by 5,000 bytes
        let (_, _, pct) = progress.add_bytes(5_000);
        assert_eq!(pct, 100);
        assert_eq!(progress.current_percent(), 100);
    }

    #[test]
    fn sync_from_snapshot_reconciles_total_and_completed_bytes() {
        let progress = JobByteProgress::new(5_000, 2_000);
        assert_eq!(progress.current_percent(), 40);

        // Advance in memory during block
        progress.add_bytes(1_000);
        assert_eq!(progress.current_percent(), 60);

        // Snapshot arrives from DB at block finalization
        progress.sync_from_snapshot(10_000, 3_000);
        assert_eq!(progress.total_bytes(), 10_000);
        assert_eq!(progress.completed_bytes(), 3_000);
        assert_eq!(progress.current_percent(), 30);
    }
}
