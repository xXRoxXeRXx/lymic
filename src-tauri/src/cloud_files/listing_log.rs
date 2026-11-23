//! Writing down what every listing and every path lookup returned (ADR-0003, #266).
//!
//! A directory that shows up empty in Explorer, or a file that cannot be found for its thumbnail,
//! leaves no trace unless somebody writes it down: the failure is a plain "not found" that nobody
//! is told about. [`ListingLog`] wraps a [`Library`] and says, in `lymic-virtual-library.log`, what
//! each directory listing returned (with its counts) and why a listing or lookup found nothing,
//! including what Immich itself reports for the directory ([`Library::explain_missing`]).

use super::diagnostic_log::{Notes, SharedNotes, SystemNotes};
use super::library::Library;
use crate::immich::RemoteAssetId;
use crate::virtual_library::{Entry, EntryKind, VirtualLibraryError};
use std::sync::Arc;

pub(super) struct ListingLog<L> {
    inner: L,
    notes: SharedNotes,
}

impl<L> ListingLog<L> {
    /// Writes to the diagnostic log of the running app.
    pub(super) fn new(inner: L) -> Self {
        Self::with_notes(inner, Arc::new(SystemNotes))
    }

    pub(super) fn with_notes(inner: L, notes: SharedNotes) -> Self {
        Self { inner, notes }
    }
}

/// `3 entries (1 directories, 2 files)`.
fn counts(entries: &[Entry]) -> String {
    let directories = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Directory)
        .count();
    format!(
        "{} entries ({directories} directories, {} files)",
        entries.len(),
        entries.len() - directories
    )
}

impl<L: Library> ListingLog<L> {
    /// What went wrong, with what Immich says about the path when it is not simply unreachable.
    async fn describe_failure(&self, path: &str, error: &VirtualLibraryError) -> String {
        match error {
            VirtualLibraryError::Remote(failure) => {
                format!("Immich: {:?}: {}", failure.kind, failure.message)
            }
            other => {
                let why = self
                    .inner
                    .explain_missing(path)
                    .await
                    .map(|why| format!(": {why}"))
                    .unwrap_or_default();
                format!("{other:?}{why}")
            }
        }
    }
}

impl<L: Library> Library for ListingLog<L> {
    async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
        let result = self.inner.list(path).await;
        match &result {
            Ok(entries) if entries.is_empty() => self
                .notes
                .note(&format!("listed '{path}': {}", counts(entries))),
            Ok(entries) => self
                .notes
                .note(&format!("listed '{path}': {}", counts(entries))),
            Err(error) => {
                let why = self.describe_failure(path, error).await;
                self.notes.note(&format!("listing '{path}' failed: {why}"));
            }
        }
        result
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        let result = self.inner.resolve(path).await;
        if let Err(error) = &result {
            let why = self.describe_failure(path, error).await;
            self.notes.note(&format!("resolving '{path}' failed: {why}"));
        }
        result
    }

    async fn explain_missing(&self, path: &str) -> Option<String> {
        self.inner.explain_missing(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::RemoteFailure;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorded(Mutex<Vec<String>>);

    impl Notes for Recorded {
        fn note(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    struct Fixed {
        listing: Result<Vec<Entry>, VirtualLibraryError>,
        explanation: Option<String>,
    }

    impl Library for Fixed {
        async fn list(&self, _path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.listing.clone()
        }

        async fn resolve(&self, _path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            self.listing.clone().map(|_| RemoteAssetId::new("a"))
        }

        async fn explain_missing(&self, _path: &str) -> Option<String> {
            self.explanation.clone()
        }
    }

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            name: name.to_string(),
            kind,
            remote_asset: None,
            size: Some(1),
            created: None,
            modified: None,
        }
    }

    fn logged(listing: Result<Vec<Entry>, VirtualLibraryError>, why: Option<&str>) -> (ListingLog<Fixed>, Arc<Recorded>) {
        let notes = Arc::new(Recorded::default());
        let library = ListingLog::with_notes(
            Fixed {
                listing,
                explanation: why.map(str::to_string),
            },
            notes.clone(),
        );
        (library, notes)
    }

    #[tokio::test]
    async fn a_listing_is_written_down_with_its_path_and_counts_and_returned_unchanged() {
        let entries = vec![
            entry("04 April", EntryKind::Directory),
            entry("a.jpg", EntryKind::File),
            entry("b.jpg", EntryKind::File),
        ];
        let (library, notes) = logged(Ok(entries.clone()), None);

        assert_eq!(library.list("All Photos/2016").await.unwrap(), entries);

        assert_eq!(
            *notes.0.lock().unwrap(),
            ["listed 'All Photos/2016': 3 entries (1 directories, 2 files)"]
        );
    }

    #[tokio::test]
    async fn a_listing_that_is_empty_says_so_with_its_path() {
        let (library, notes) = logged(Ok(vec![]), None);

        library.list("Albums/Empty").await.unwrap();

        assert_eq!(
            *notes.0.lock().unwrap(),
            ["listed 'Albums/Empty': 0 entries (0 directories, 0 files)"]
        );
    }

    #[tokio::test]
    async fn a_directory_that_was_not_found_says_what_immich_reports_for_it() {
        let (library, notes) = logged(
            Err(VirtualLibraryError::NotFound),
            Some("the timeline reports 3 asset(s) for 2016-04, the search returned 0 asset(s)"),
        );

        assert_eq!(
            library.list("All Photos/2016/04 April").await,
            Err(VirtualLibraryError::NotFound)
        );

        let lines = notes.0.lock().unwrap().clone();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("'All Photos/2016/04 April'"), "{lines:?}");
        assert!(lines[0].contains("NotFound"), "{lines:?}");
        assert!(lines[0].contains("reports 3 asset(s)"), "{lines:?}");
    }

    #[tokio::test]
    async fn a_failed_lookup_is_written_down_with_the_path_and_the_reason() {
        let (library, notes) = logged(
            Err(VirtualLibraryError::Remote(RemoteFailure::unavailable(
                "error sending request",
            ))),
            None,
        );

        assert!(library.resolve("All Photos/2016/04 April/a.jpg").await.is_err());

        let lines = notes.0.lock().unwrap().clone();
        assert!(lines[0].contains("resolving 'All Photos/2016/04 April/a.jpg' failed"), "{lines:?}");
        assert!(lines[0].contains("error sending request"), "{lines:?}");
    }

    #[tokio::test]
    async fn a_lookup_that_works_writes_nothing() {
        let (library, notes) = logged(Ok(vec![]), None);

        library.resolve("a.jpg").await.unwrap();

        assert!(notes.0.lock().unwrap().is_empty());
    }
}
