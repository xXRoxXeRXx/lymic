//! When the out-of-process host may exit.
//!
//! COM starts `lymic-shell-ext-host.exe` when Explorer first needs a thumbnail and never tells it
//! to stop. The host stays while any provider object is alive or while it was used recently
//! (Explorer asks for thumbnails in bursts), and leaves otherwise, so it does not keep running for
//! a user who is not browsing the Virtual Library.

use std::time::Duration;

/// How long an unused host waits for the first or the next request.
pub const GRACE: Duration = Duration::from_secs(30);

/// Whether the host should exit: no provider object is alive and nothing has happened for
/// [`GRACE`].
pub fn should_exit(live_objects: usize, idle_for: Duration) -> bool {
    live_objects == 0 && idle_for >= GRACE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_stays_while_an_object_is_alive_however_long_it_is_idle() {
        assert!(!should_exit(1, Duration::from_secs(3600)));
    }

    #[test]
    fn the_host_stays_during_the_grace_period_after_the_last_activity() {
        assert!(!should_exit(0, GRACE - Duration::from_millis(1)));
    }

    #[test]
    fn the_host_exits_when_nothing_is_alive_and_the_grace_period_is_over() {
        assert!(should_exit(0, GRACE));
    }
}
