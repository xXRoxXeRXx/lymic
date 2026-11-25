//! One reconciliation run over every already-populated directory (ADR-0003, #280).
//!
//! For each directory recorded as populated, parents before children, the directory is listed again
//! through the projection (`population::populate`, the same listing Explorer's first visit used),
//! compared with the placeholders on disk (`diff::diff`) and the difference applied. A directory
//! that was never populated is never listed here: it is listed when somebody first opens it, and an
//! empty directory placeholder costs nothing until then.
//!
//! A failed listing is never read as "the directory is empty": the run records the failure and
//! leaves that directory alone, and stops altogether when Immich is unreachable or the credentials
//! are rejected, so an outage cannot delete anything.
//!
//! The disk and the record of populated directories sit behind traits; the Windows implementation
//! is in `adapter::reconcile`.

use super::diff::{self, LocalEntry, Plan, Removal};
use super::failure::FailureKind;
use super::identity;
use super::library::{Library, SizeLookup};
use super::population;
use std::future::Future;

/// Why a directory on disk could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalError {
    /// The directory is not there (any more).
    Missing,
    Other(String),
}

/// What applying a [`Plan`] to disk achieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Applied {
    pub created: usize,
    pub updated: usize,
    /// The removals that really happened. A directory placeholder that still held something that is
    /// not Lymic's stays and is not listed.
    pub removed: Vec<Removal>,
    /// Descriptions of what could not be applied; the next run tries again.
    pub failed: Vec<String>,
}

/// The directories on disk, as the adapter sees them.
pub(super) trait LocalTree: Sync {
    /// The items in the directory at `path` (`""` is the root). Never populates it.
    fn list(&self, path: &str) -> impl Future<Output = Result<Vec<LocalEntry>, LocalError>> + Send;

    /// Applies `plan` to the directory at `path`.
    fn apply(&self, path: &str, plan: &Plan) -> impl Future<Output = Applied> + Send;
}

/// The record of directories that were populated (listed once and handed to the platform).
pub(super) trait PopulatedDirectories: Sync {
    fn all(&self) -> impl Future<Output = Result<Vec<String>, String>> + Send;

    /// Forgets `path` and every directory below it, for when the directory is gone.
    fn forget_tree(&self, path: &str) -> impl Future<Output = Result<(), String>> + Send;
}

/// Whether the run may still change the disk. Cleanup (disable, logout) can begin while a run is
/// listing from Immich; a plan made before that must not be applied to a root being cleaned up.
pub(super) trait RunGate: Sync {
    fn still_enabled(&self) -> impl Future<Output = bool> + Send;
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Directories listed and compared.
    pub directories: usize,
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
    /// The first thing that went wrong, if anything. The run carried on with the other directories
    /// unless it was Immich being unreachable or rejecting the credentials.
    pub failure: Option<String>,
}

impl Report {
    fn fail(&mut self, message: String) {
        self.failure.get_or_insert(message);
    }
}

/// Number of path segments; the root is 0.
fn depth(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split('/').count()
    }
}

/// Whether `path` is `ancestor` or lies below it.
fn is_within(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Reconciles every populated directory once.
pub(super) async fn reconcile<L, S, T, P, G>(
    library: &L,
    sizes: &S,
    tree: &T,
    populated: &P,
    gate: &G,
) -> Report
where
    L: Library,
    S: SizeLookup,
    T: LocalTree,
    P: PopulatedDirectories,
    G: RunGate,
{
    let mut report = Report::default();
    let mut directories = match populated.all().await {
        Ok(directories) => directories,
        Err(error) => {
            report.fail(format!("could not read the populated directories: {error}"));
            return report;
        }
    };
    directories.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
    let mut removed_trees: Vec<String> = Vec::new();

    for path in directories {
        if removed_trees.iter().any(|gone| is_within(&path, gone)) {
            // Its directory was removed above; the record of it goes too.
            forget(populated, &path, &mut report).await;
            continue;
        }
        let desired = match population::populate(library, sizes, &path).await {
            Ok(plan) => plan,
            // The parent has not been diffed away yet, or the directory vanished a moment ago; the
            // next run sees it from the parent's side. Nothing here is a reason to delete.
            Err(failure) if failure.kind == FailureKind::NotFound => continue,
            Err(failure) => {
                let stop = matches!(
                    failure.kind,
                    FailureKind::Offline | FailureKind::Unauthorized
                );
                report.fail(format!("could not list '{path}': {}", failure.message));
                if stop {
                    break;
                }
                continue;
            }
        };
        let local = match tree.list(&path).await {
            Ok(local) => local,
            Err(LocalError::Missing) => {
                // Nothing on disk to reconcile; the platform will list it when it is opened again.
                forget(populated, &path, &mut report).await;
                continue;
            }
            Err(LocalError::Other(error)) => {
                report.fail(format!("could not read '{path}' on disk: {error}"));
                continue;
            }
        };
        report.directories += 1;
        let plan = diff::diff(&path, &desired.specs, &desired.skipped, &local);
        if plan.is_empty() {
            continue;
        }
        // Re-checked right before every change: the listing above can take a while, and a cleanup
        // that began meanwhile owns the folder now.
        if !gate.still_enabled().await {
            break;
        }
        let applied = tree.apply(&path, &plan).await;
        report.created += applied.created;
        report.updated += applied.updated;
        report.removed += applied.removed.len();
        for message in applied.failed {
            report.fail(format!("in '{path}': {message}"));
        }
        for removal in applied
            .removed
            .iter()
            .filter(|removal| removal.is_directory)
        {
            let gone = identity::child_path(&path, &removal.name);
            forget(populated, &gone, &mut report).await;
            removed_trees.push(gone);
        }
    }
    report
}

async fn forget<P: PopulatedDirectories>(populated: &P, path: &str, report: &mut Report) {
    if let Err(error) = populated.forget_tree(path).await {
        report.fail(format!("could not forget '{path}': {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_files::diff::LocalState;
    use crate::cloud_files::placeholder::PlaceholderSpec;
    use crate::immich::{RemoteAssetId, RemoteFailure};
    use crate::virtual_library::{Entry, EntryKind, VirtualLibraryError};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    fn file(name: &str, id: &str, size: u64) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::File,
            remote_asset: Some(RemoteAssetId::new(id)),
            size: Some(size),
            created: None,
            modified: None,
        }
    }

    fn directory(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::Directory,
            remote_asset: None,
            size: None,
            created: None,
            modified: None,
        }
    }

    /// Immich as the projection shows it; tests change it between runs.
    #[derive(Default)]
    struct FakeLibrary {
        directories: Mutex<HashMap<String, Result<Vec<Entry>, VirtualLibraryError>>>,
        listed: Mutex<Vec<String>>,
    }

    impl FakeLibrary {
        fn set(&self, path: &str, entries: Result<Vec<Entry>, VirtualLibraryError>) {
            self.directories
                .lock()
                .unwrap()
                .insert(path.to_string(), entries);
        }
    }

    impl Library for FakeLibrary {
        async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.listed.lock().unwrap().push(path.to_string());
            self.directories
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .unwrap_or(Err(VirtualLibraryError::NotFound))
        }

        async fn resolve(&self, _path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            unreachable!("reconciliation never resolves")
        }
    }

    struct NoSizes;

    impl SizeLookup for NoSizes {
        async fn size_of(&self, _asset: &RemoteAssetId) -> Option<u64> {
            None
        }
    }

    /// A disk that applies plans the way the adapter should: create, update and remove placeholders,
    /// but keep a directory that holds something that is not Lymic's.
    #[derive(Default)]
    struct FakeTree {
        directories: Mutex<BTreeMap<String, Vec<LocalEntry>>>,
        read_error: Mutex<Option<LocalError>>,
    }

    impl FakeTree {
        fn put(&self, path: &str, entries: Vec<LocalEntry>) {
            self.directories
                .lock()
                .unwrap()
                .insert(path.to_string(), entries);
        }

        fn names(&self, path: &str) -> Vec<String> {
            let mut names = self.directories.lock().unwrap()[path]
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>();
            names.sort();
            names
        }
    }

    fn local_from(parent: &str, spec: &PlaceholderSpec) -> LocalEntry {
        LocalEntry {
            name: spec.name.clone(),
            is_directory: spec.is_directory,
            state: LocalState::Placeholder {
                identity: Some(identity::child_path(parent, &spec.name)),
                in_sync: true,
            },
            size: spec.size,
            created: spec.created,
            modified: spec.modified,
        }
    }

    impl LocalTree for FakeTree {
        async fn list(&self, path: &str) -> Result<Vec<LocalEntry>, LocalError> {
            if let Some(error) = self.read_error.lock().unwrap().clone() {
                return Err(error);
            }
            self.directories
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(LocalError::Missing)
        }

        async fn apply(&self, path: &str, plan: &Plan) -> Applied {
            let mut directories = self.directories.lock().unwrap();
            let mut applied = Applied::default();
            for removal in &plan.remove {
                let child = identity::child_path(path, &removal.name);
                let holds_foreign = directories
                    .get(&child)
                    .is_some_and(|inner| inner.iter().any(|e| e.state == LocalState::Foreign));
                if holds_foreign {
                    continue;
                }
                directories
                    .get_mut(path)
                    .unwrap()
                    .retain(|entry| entry.name != removal.name);
                directories.retain(|key, _| !is_within(key, &child));
                applied.removed.push(removal.clone());
            }
            for update in &plan.update {
                let entries = directories.get_mut(path).unwrap();
                let slot = entries
                    .iter_mut()
                    .find(|entry| entry.name == update.spec.name)
                    .unwrap();
                *slot = local_from(path, &update.spec);
                applied.updated += 1;
            }
            for spec in &plan.create {
                directories
                    .get_mut(path)
                    .unwrap()
                    .push(local_from(path, spec));
                applied.created += 1;
            }
            applied
        }
    }

    struct FakePopulated {
        paths: Mutex<Vec<String>>,
        fail: bool,
    }

    impl FakePopulated {
        fn of(paths: &[&str]) -> Self {
            Self {
                paths: Mutex::new(paths.iter().map(|path| path.to_string()).collect()),
                fail: false,
            }
        }

        fn paths(&self) -> Vec<String> {
            let mut paths = self.paths.lock().unwrap().clone();
            paths.sort();
            paths
        }
    }

    impl PopulatedDirectories for FakePopulated {
        async fn all(&self) -> Result<Vec<String>, String> {
            if self.fail {
                return Err("database is locked".to_string());
            }
            Ok(self.paths.lock().unwrap().clone())
        }

        async fn forget_tree(&self, path: &str) -> Result<(), String> {
            self.paths
                .lock()
                .unwrap()
                .retain(|known| !is_within(known, path));
            Ok(())
        }
    }

    /// The Virtual Library stays on for the whole run.
    struct Enabled;

    impl RunGate for Enabled {
        async fn still_enabled(&self) -> bool {
            true
        }
    }

    /// The Virtual Library is turned off after `answers` checks: the first `answers` say on.
    struct TurnsOff {
        answers: Mutex<usize>,
    }

    impl RunGate for TurnsOff {
        async fn still_enabled(&self) -> bool {
            let mut left = self.answers.lock().unwrap();
            if *left == 0 {
                return false;
            }
            *left -= 1;
            true
        }
    }

    /// A library with `Albums` holding two albums, each populated on disk with one photo.
    fn world() -> (FakeLibrary, FakeTree) {
        let library = FakeLibrary::default();
        library.set("", Ok(vec![directory("Albums")]));
        library.set("Albums", Ok(vec![directory("Trip"), directory("Home")]));
        library.set("Albums/Trip", Ok(vec![file("a.jpg", "id-a", 10)]));
        library.set("Albums/Home", Ok(vec![file("h.jpg", "id-h", 5)]));
        let tree = FakeTree::default();
        for (path, entries) in [
            ("", vec![directory("Albums")]),
            ("Albums", vec![directory("Trip"), directory("Home")]),
            ("Albums/Trip", vec![file("a.jpg", "id-a", 10)]),
            ("Albums/Home", vec![file("h.jpg", "id-h", 5)]),
        ] {
            let desired = crate::cloud_files::placeholder::plan(path, &entries);
            tree.put(
                path,
                desired.specs.iter().map(|s| local_from(path, s)).collect(),
            );
        }
        (library, tree)
    }

    const POPULATED: [&str; 4] = ["", "Albums", "Albums/Trip", "Albums/Home"];

    #[tokio::test]
    async fn a_new_remote_asset_appears_in_a_populated_directory() {
        let (library, tree) = world();
        library.set(
            "Albums/Trip",
            Ok(vec![file("a.jpg", "id-a", 10), file("b.jpg", "id-b", 20)]),
        );

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &Enabled,
        )
        .await;

        assert_eq!(tree.names("Albums/Trip"), vec!["a.jpg", "b.jpg"]);
        assert_eq!((report.created, report.updated, report.removed), (1, 0, 0));
        assert_eq!(report.failure, None);
    }

    #[tokio::test]
    async fn a_remotely_removed_asset_disappears() {
        let (library, tree) = world();
        library.set("Albums/Trip", Ok(vec![]));

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &Enabled,
        )
        .await;

        assert!(tree.names("Albums/Trip").is_empty());
        assert_eq!(report.removed, 1);
    }

    #[tokio::test]
    async fn a_changed_remote_asset_is_updated_in_place() {
        let (library, tree) = world();
        library.set("Albums/Trip", Ok(vec![file("a.jpg", "id-a", 99)]));

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &Enabled,
        )
        .await;

        assert_eq!(tree.directories.lock().unwrap()["Albums/Trip"][0].size, 99);
        assert_eq!(report.updated, 1);
    }

    #[tokio::test]
    async fn a_run_that_finds_the_virtual_library_turned_off_applies_nothing_more() {
        let (library, tree) = world();
        library.set(
            "Albums/Trip",
            Ok(vec![file("a.jpg", "id-a", 10), file("b.jpg", "id-b", 20)]),
        );
        library.set("Albums/Home", Ok(vec![]));
        // Cleanup began after the listing of the first directories: nothing may be created in, or
        // removed from, a root that is being (or has been) cleaned up.
        let gate = TurnsOff {
            answers: Mutex::new(0),
        };

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &gate,
        )
        .await;

        assert_eq!(tree.names("Albums/Trip"), vec!["a.jpg"]);
        assert_eq!(tree.names("Albums/Home"), vec!["h.jpg"]);
        assert_eq!((report.created, report.updated, report.removed), (0, 0, 0));
        assert_eq!(report.failure, None, "being turned off is not a failure");
    }

    #[tokio::test]
    async fn only_populated_directories_are_listed_or_diffed() {
        let (library, tree) = world();
        library.set("Albums/Home", Ok(vec![file("new.jpg", "id-n", 1)]));
        library.set("Albums/Trip", Ok(vec![file("new.jpg", "id-n", 1)]));

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&["", "Albums", "Albums/Trip"]),
            &Enabled,
        )
        .await;

        assert_eq!(
            *library.listed.lock().unwrap(),
            vec!["", "Albums", "Albums/Trip"],
            "Albums/Home was never populated, so it is not even listed"
        );
        assert_eq!(tree.names("Albums/Home"), vec!["h.jpg"]);
        assert_eq!(report.directories, 3);
    }

    #[tokio::test]
    async fn a_removed_album_takes_its_directory_and_its_populated_record_with_it() {
        let (library, tree) = world();
        library.set("Albums", Ok(vec![directory("Home")]));
        library.set("Albums/Trip", Err(VirtualLibraryError::NotFound));
        let populated = FakePopulated::of(&POPULATED);

        let report = reconcile(&library, &NoSizes, &tree, &populated, &Enabled).await;

        assert_eq!(tree.names("Albums"), vec!["Home"]);
        assert_eq!(populated.paths(), vec!["", "Albums", "Albums/Home"]);
        assert_eq!(report.removed, 1);
        assert_eq!(report.failure, None);
    }

    #[tokio::test]
    async fn parents_are_reconciled_before_their_children() {
        let (library, tree) = world();

        reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&["Albums/Trip", "Albums", "", "Albums/Home"]),
            &Enabled,
        )
        .await;

        let listed = library.listed.lock().unwrap().clone();
        assert_eq!(listed[..2], ["", "Albums"]);
    }

    #[tokio::test]
    async fn an_unreachable_server_stops_the_run_and_removes_nothing() {
        let (library, tree) = world();
        library.set(
            "Albums",
            Err(VirtualLibraryError::Remote(RemoteFailure::unavailable(
                "error sending request",
            ))),
        );

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &Enabled,
        )
        .await;

        assert_eq!(tree.names("Albums"), vec!["Home", "Trip"]);
        assert_eq!(report.directories, 1, "only the root was compared");
        assert!(report.failure.unwrap().contains("'Albums'"));
        assert_eq!(*library.listed.lock().unwrap(), vec!["", "Albums"]);
    }

    #[tokio::test]
    async fn a_server_error_in_one_directory_does_not_stop_the_others() {
        let (library, tree) = world();
        library.set(
            "Albums/Trip",
            Err(VirtualLibraryError::Remote(RemoteFailure::rejected(
                "request failed with status 500",
            ))),
        );
        library.set("Albums/Home", Ok(vec![]));

        let report = reconcile(
            &library,
            &NoSizes,
            &tree,
            &FakePopulated::of(&POPULATED),
            &Enabled,
        )
        .await;

        assert_eq!(tree.names("Albums/Trip"), vec!["a.jpg"], "left alone");
        assert!(tree.names("Albums/Home").is_empty());
        assert!(report.failure.unwrap().contains("Albums/Trip"));
    }

    #[tokio::test]
    async fn a_directory_missing_on_disk_is_forgotten_and_the_run_goes_on() {
        let (library, tree) = world();
        tree.directories.lock().unwrap().remove("Albums/Trip");
        let populated = FakePopulated::of(&POPULATED);

        let report = reconcile(&library, &NoSizes, &tree, &populated, &Enabled).await;

        assert_eq!(populated.paths(), vec!["", "Albums", "Albums/Home"]);
        assert_eq!(report.failure, None);
    }

    #[tokio::test]
    async fn a_disk_that_cannot_be_read_is_reported_and_changes_nothing() {
        let (library, tree) = world();
        *tree.read_error.lock().unwrap() = Some(LocalError::Other("access denied".to_string()));
        let populated = FakePopulated::of(&POPULATED);

        let report = reconcile(&library, &NoSizes, &tree, &populated, &Enabled).await;

        assert_eq!(populated.paths().len(), 4);
        assert!(report.failure.unwrap().contains("access denied"));
        assert_eq!(report.directories, 0);
    }

    #[tokio::test]
    async fn nothing_populated_means_nothing_is_listed() {
        let (library, tree) = world();

        let report = reconcile(&library, &NoSizes, &tree, &FakePopulated::of(&[]), &Enabled).await;

        assert!(library.listed.lock().unwrap().is_empty());
        assert_eq!(report, Report::default());
    }

    #[tokio::test]
    async fn an_unreadable_record_fails_the_run_without_touching_anything() {
        let (library, tree) = world();
        let populated = FakePopulated {
            fail: true,
            ..FakePopulated::of(&POPULATED)
        };

        let report = reconcile(&library, &NoSizes, &tree, &populated, &Enabled).await;

        assert!(library.listed.lock().unwrap().is_empty());
        assert!(report.failure.unwrap().contains("database is locked"));
    }

    #[tokio::test]
    async fn a_directory_that_still_holds_the_users_files_stays_and_stays_recorded() {
        let (library, tree) = world();
        library.set("Albums", Ok(vec![directory("Home")]));
        library.set("Albums/Trip", Err(VirtualLibraryError::NotFound));
        tree.directories
            .lock()
            .unwrap()
            .get_mut("Albums/Trip")
            .unwrap()
            .push(LocalEntry {
                name: "mine.txt".to_string(),
                is_directory: false,
                state: LocalState::Foreign,
                size: 1,
                created: 0,
                modified: 0,
            });
        let populated = FakePopulated::of(&POPULATED);

        let report = reconcile(&library, &NoSizes, &tree, &populated, &Enabled).await;

        assert_eq!(report.removed, 0);
        assert!(tree.directories.lock().unwrap().contains_key("Albums/Trip"));
        assert!(populated.paths().contains(&"Albums/Trip".to_string()));
    }
}
