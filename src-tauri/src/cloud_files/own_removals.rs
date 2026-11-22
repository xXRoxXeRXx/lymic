//! The placeholders Lymic itself is deleting during reconciliation (#280).
//!
//! Explorer's delete and rename of a Virtual Placeholder are vetoed (#279), and the platform may
//! announce Lymic's own deletion of a placeholder through the same `NOTIFY_DELETE` callback. The
//! veto must let those through, or a remotely removed asset could never leave the disk. While
//! reconciliation deletes a placeholder it registers its path here; the delete callback asks
//! [`OwnRemovals::is_own`] and acknowledges instead of vetoing when the answer is yes.
//!
//! Paths are root-relative with `/`, the same string the placeholder's file identity holds, and are
//! compared as they are.

use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct OwnRemovals {
    paths: Mutex<BTreeSet<String>>,
}

/// Registered for as long as it lives; dropping it ends the registration.
pub(super) struct Removing<'a> {
    owner: &'a OwnRemovals,
    path: String,
}

impl OwnRemovals {
    pub(super) const fn new() -> Self {
        Self {
            paths: Mutex::new(BTreeSet::new()),
        }
    }

    /// Registers `path` as being deleted by Lymic itself.
    pub(super) fn begin(&self, path: &str) -> Removing<'_> {
        if let Ok(mut paths) = self.paths.lock() {
            paths.insert(path.to_string());
        }
        Removing {
            owner: self,
            path: path.to_string(),
        }
    }

    /// Whether Lymic itself is deleting the placeholder at `path` right now.
    pub(super) fn is_own(&self, path: &str) -> bool {
        self.paths
            .lock()
            .map(|paths| paths.contains(path))
            .unwrap_or(false)
    }
}

impl Drop for Removing<'_> {
    fn drop(&mut self) {
        if let Ok(mut paths) = self.owner.paths.lock() {
            paths.remove(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_lymics_own_removal_only_while_it_is_being_removed() {
        let removals = OwnRemovals::new();
        assert!(!removals.is_own("Albums/Trip/a.jpg"));

        let removing = removals.begin("Albums/Trip/a.jpg");
        assert!(removals.is_own("Albums/Trip/a.jpg"));
        assert!(!removals.is_own("Albums/Trip/b.jpg"), "only that path");

        drop(removing);
        assert!(!removals.is_own("Albums/Trip/a.jpg"));
    }

    #[test]
    fn overlapping_removals_are_tracked_independently() {
        let removals = OwnRemovals::new();
        let first = removals.begin("a");
        let second = removals.begin("b");

        drop(first);

        assert!(!removals.is_own("a"));
        assert!(removals.is_own("b"));
        drop(second);
    }
}
