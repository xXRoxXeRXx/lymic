//! Removing everything the Virtual Library created, and nothing else (ADR-0003, #283).
//!
//! Disabling the Virtual Library or logging out runs [`clean_up`]: stop serving, look at what is
//! in the root folder, unregister the sync root, then delete what Lymic created. The deciding is
//! pure and tested on every platform; the Windows calls (reading the placeholder state, WinRT
//! `Unregister`, `CfDisconnectSyncRoot`) sit behind [`CleanupHost`] in `adapter::cleanup`.
//!
//! What counts as created by Lymic: an item is Lymic's only when the platform reports it as a
//! placeholder **and** its file identity is exactly the root-relative path Lymic stored there
//! (`identity`) **and**, for a file, it is still in sync, so nobody changed it locally. A hydrated or
//! pinned original is still such a placeholder; that is why it is removed. Everything else is left
//! alone: a plain file, a plain folder (the user made it), a placeholder the user edited or moved
//! out of its place. A folder is removed only when everything in it is removed; removal never
//! recurses, so a file that appears between the look and the delete makes the delete of its folder
//! fail instead of being deleted with it.
//!
//! The look happens **before** the sync root is unregistered, because what the platform does to
//! hydrated placeholders on unregistering is not established (the gate prototype saw them turn into
//! plain files, unpackaged and on Windows Server only), and a hydrated original that has turned
//! plain can no longer be told from a user file. That makes the cleanup safe in both outcomes: if
//! the platform leaves placeholders, they are removed; if it turned them plain before a later run,
//! they are kept and reported, never deleted. For the same reason a second run after the first
//! succeeded finds nothing and changes nothing, and a run after a crash works without a connection.

use super::identity;
use serde::Serialize;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// How many items a report names; the rest are only counted.
const EXAMPLE_LIMIT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Directory,
}

/// What the platform says about an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Observed {
    /// Not a placeholder: a file or folder somebody else created.
    Plain,
    /// A Cloud Files placeholder, hydrated or not, pinned or not.
    Placeholder { identity: Vec<u8>, in_sync: bool },
}

/// What listing a directory gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Children {
    /// A file.
    None,
    Listed(Vec<Node>),
    /// The directory could not be listed (a placeholder directory the platform cannot populate
    /// while nothing serves it, for example). Whatever is in it is unknown.
    Unreadable(String),
}

/// One item of the tree under the root, as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) name: OsString,
    pub(super) kind: Kind,
    pub(super) observed: Observed,
    pub(super) children: Children,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum KeptReason {
    /// Not a placeholder: not created by Lymic.
    NotCreatedByLymic,
    /// A placeholder the user changed locally; the changes are theirs.
    ChangedLocally,
    /// A placeholder that is not where Lymic put it.
    NotAtItsPlace,
    /// A folder that still held something when it was removed.
    HoldsOtherItems,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Kept {
    pub(super) path: PathBuf,
    pub(super) kind: Kind,
    pub(super) reason: KeptReason,
}

/// What to delete (paths relative to the root, folders deepest first, the root itself as the empty
/// path last) and what is left alone.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Plan {
    pub(super) remove_files: Vec<PathBuf>,
    pub(super) remove_folders: Vec<PathBuf>,
    pub(super) kept: Vec<Kept>,
}

impl Plan {
    #[cfg(test)]
    pub(super) fn removes_root(&self) -> bool {
        self.remove_folders
            .iter()
            .any(|path| path.as_os_str().is_empty())
    }
}

enum Ownership {
    Lymic,
    Other(KeptReason),
}

fn ownership(node: &Node, path: Option<&str>) -> Ownership {
    let Observed::Placeholder {
        identity: stored,
        in_sync,
    } = &node.observed
    else {
        return Ownership::Other(KeptReason::NotCreatedByLymic);
    };
    let at_its_place = path
        .and_then(identity::encode)
        .is_some_and(|expected| &expected == stored);
    if !at_its_place {
        return Ownership::Other(KeptReason::NotAtItsPlace);
    }
    // A folder's state follows what is inside it; what is inside is judged item by item.
    if node.kind == Kind::File && !in_sync {
        return Ownership::Other(KeptReason::ChangedLocally);
    }
    Ownership::Lymic
}

/// Decides what to delete. `root` is the root folder itself.
pub(super) fn plan(root: &Node) -> Plan {
    let mut plan = Plan::default();
    let removable = visit_children(root, Some(""), PathBuf::new(), &mut plan);
    if removable {
        plan.remove_folders.push(PathBuf::new());
    }
    plan
}

/// Visits what is in `directory` and says whether all of it is going to be removed.
fn visit_children(
    directory: &Node,
    path: Option<&str>,
    relative: PathBuf,
    plan: &mut Plan,
) -> bool {
    match &directory.children {
        Children::None => true,
        // Unknown contents: try to remove the folder anyway. Removal never recurses, so this only
        // succeeds when nothing is in it.
        Children::Unreadable(_) => true,
        Children::Listed(children) => {
            let mut all_removed = true;
            for child in children {
                let child_relative = relative.join(&child.name);
                let child_path = match (path, child.name.to_str()) {
                    (Some(parent), Some(name)) => Some(identity::child_path(parent, name)),
                    _ => None,
                };
                all_removed &= visit(child, child_path.as_deref(), child_relative, plan);
            }
            all_removed
        }
    }
}

fn visit(node: &Node, path: Option<&str>, relative: PathBuf, plan: &mut Plan) -> bool {
    let owned = ownership(node, path);
    let children_removed = match node.kind {
        Kind::File => true,
        Kind::Directory => visit_children(node, path, relative.clone(), plan),
    };
    match owned {
        Ownership::Other(reason) => {
            plan.kept.push(Kept {
                path: relative,
                kind: node.kind,
                reason,
            });
            false
        }
        Ownership::Lymic => match node.kind {
            Kind::File => {
                plan.remove_files.push(relative);
                true
            }
            Kind::Directory if children_removed => {
                plan.remove_folders.push(relative);
                true
            }
            Kind::Directory => false,
        },
    }
}

// ------------------------------------------------------------------ executing a plan

/// The deletions, behind a trait so a failure half way is testable.
pub(super) trait CleanupFs {
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// Removes an empty folder. Never recursive.
    fn remove_folder(&self, path: &Path) -> io::Result<()>;
}

/// The real filesystem.
pub(super) struct StdFs;

impl CleanupFs for StdFs {
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                // A read-only attribute stops the delete; it is Lymic's own file at this point.
                let mut permissions = std::fs::metadata(path)?.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                std::fs::set_permissions(path, permissions)?;
                std::fs::remove_file(path)
            }
            other => other,
        }
    }

    fn remove_folder(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir(path)
    }
}

/// What a cleanup did. Sent to the frontend, which tells the user when something is left.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupReport {
    /// The Virtual Library folder that was cleaned.
    pub(crate) root_folder: String,
    pub(crate) removed_files: usize,
    pub(crate) removed_folders: usize,
    /// Items left in place because Lymic did not create them (or the user changed them).
    pub(crate) kept_count: usize,
    /// The first few of them, relative to the root folder.
    pub(crate) kept_examples: Vec<String>,
    /// The root folder itself stays, because something is left in it.
    pub(crate) root_kept: bool,
    /// Items that could not be removed.
    pub(crate) failed_count: usize,
    /// The first few of them with the reason.
    pub(crate) failed_examples: Vec<String>,
    /// The cleanup could not run (the sync root could not be unregistered). Nothing was removed.
    pub(crate) error: Option<String>,
}

impl CleanupReport {
    /// Whether the user has to be told anything: something stayed, or something failed.
    pub(crate) fn needs_notice(&self) -> bool {
        self.kept_count > 0 || self.root_kept || self.failed_count > 0 || self.error.is_some()
    }

    /// A report for a cleanup that could not run at all.
    pub(crate) fn not_run(root: &Path, error: impl Into<String>) -> Self {
        Self {
            root_folder: root.to_string_lossy().into_owned(),
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

fn describe(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path.to_string_lossy().into_owned()
    }
}

/// Deletes what `plan` says, best effort: a failure is recorded and the rest goes on. Something
/// already gone counts as done, so a repeated run is harmless.
pub(super) fn execute(root: &Path, plan: Plan, fs: &impl CleanupFs) -> CleanupReport {
    let mut report = CleanupReport {
        root_folder: root.to_string_lossy().into_owned(),
        ..CleanupReport::default()
    };
    let mut failed: Vec<(PathBuf, String)> = Vec::new();
    // Items that are still there: a failed removal, or a folder that held something else.
    let mut blocked: Vec<PathBuf> = Vec::new();
    let mut kept = plan.kept;
    let mut root_removed = false;

    for relative in &plan.remove_files {
        match fs.remove_file(&root.join(relative)) {
            Ok(()) => report.removed_files += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                blocked.push(relative.clone());
                failed.push((relative.clone(), error.to_string()));
            }
        }
    }
    for relative in &plan.remove_folders {
        if blocked.iter().any(|path| path.starts_with(relative)) {
            // Something inside is still there, so this folder is not empty either.
            continue;
        }
        let is_root = relative.as_os_str().is_empty();
        match fs.remove_folder(&root.join(relative)) {
            Ok(()) => {
                report.removed_folders += 1;
                root_removed |= is_root;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => root_removed |= is_root,
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {
                blocked.push(relative.clone());
                if !is_root {
                    kept.push(Kept {
                        path: relative.clone(),
                        kind: Kind::Directory,
                        reason: KeptReason::HoldsOtherItems,
                    });
                }
            }
            Err(error) => {
                blocked.push(relative.clone());
                failed.push((relative.clone(), error.to_string()));
            }
        }
    }

    report.root_kept = !root_removed;
    report.kept_count = kept.len();
    report.kept_examples = kept
        .iter()
        .take(EXAMPLE_LIMIT)
        .map(|item| describe(&item.path))
        .collect();
    report.failed_count = failed.len();
    report.failed_examples = failed
        .iter()
        .take(EXAMPLE_LIMIT)
        .map(|(path, reason)| format!("{}: {reason}", describe(path)))
        .collect();
    report
}

// ------------------------------------------------------------------ the whole cleanup

/// The operating system side of a cleanup.
pub(super) trait CleanupHost: CleanupFs {
    /// Stops serving callbacks. Nothing to do when nothing is connected (after a crash).
    fn disconnect(&self);
    /// Looks at the root folder without reading file contents. `None` when it does not exist.
    fn scan(&self, root: &Path) -> Result<Option<Node>, String>;
    /// Removes the sync root registration. Not being registered is success.
    fn unregister(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CleanupError {
    /// The root folder could not be examined; nothing was changed.
    Scan(String),
    /// The sync root could not be unregistered; nothing was removed.
    Unregister(String),
    /// Not Windows.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

/// Error text prefix when the cleanup could not run; the text after `": "` is the technical reason.
/// The frontend turns it into a localized message.
pub(crate) const CLEANUP_FAILED_ERROR: &str = "VIRTUAL_LIBRARY_CLEANUP_FAILED";

impl CleanupError {
    /// The error text the frontend receives. It never carries credentials.
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::Scan(reason) => {
                format!("{CLEANUP_FAILED_ERROR}: could not examine the folder: {reason}")
            }
            Self::Unregister(reason) => {
                format!("{CLEANUP_FAILED_ERROR}: could not unregister the sync folder: {reason}")
            }
            Self::Unsupported => {
                format!("{CLEANUP_FAILED_ERROR}: the Virtual Library needs Windows")
            }
        }
    }
}

/// Stops serving, looks at the root folder, unregisters the sync root and removes what Lymic created.
/// Works the same when nothing is connected or registered any more. Blocking.
pub(super) fn clean_up(
    root: &Path,
    host: &impl CleanupHost,
) -> Result<CleanupReport, CleanupError> {
    host.disconnect();
    let tree = host.scan(root).map_err(CleanupError::Scan)?;
    host.unregister().map_err(CleanupError::Unregister)?;
    let Some(tree) = tree else {
        return Ok(CleanupReport {
            root_folder: root.to_string_lossy().into_owned(),
            ..CleanupReport::default()
        });
    };
    Ok(execute(root, plan(&tree), host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    fn placeholder(path: &str) -> Observed {
        Observed::Placeholder {
            identity: path.as_bytes().to_vec(),
            in_sync: true,
        }
    }

    fn file(name: &str, parent: &str) -> Node {
        let path = identity::child_path(parent, name);
        Node {
            name: name.into(),
            kind: Kind::File,
            observed: placeholder(&path),
            children: Children::None,
        }
    }

    fn user_file(name: &str) -> Node {
        Node {
            name: name.into(),
            kind: Kind::File,
            observed: Observed::Plain,
            children: Children::None,
        }
    }

    fn folder(name: &str, parent: &str, children: Vec<Node>) -> Node {
        let path = identity::child_path(parent, name);
        Node {
            name: name.into(),
            kind: Kind::Directory,
            observed: placeholder(&path),
            children: Children::Listed(children),
        }
    }

    fn root(children: Vec<Node>) -> Node {
        Node {
            name: OsString::new(),
            kind: Kind::Directory,
            observed: Observed::Plain,
            children: Children::Listed(children),
        }
    }

    fn library() -> Node {
        root(vec![
            folder(
                "All Photos",
                "",
                vec![folder(
                    "2024",
                    "All Photos",
                    vec![
                        file("a.jpg", "All Photos/2024"),
                        file("b.jpg", "All Photos/2024"),
                    ],
                )],
            ),
            folder("Albums", "", vec![]),
        ])
    }

    fn paths(items: &[PathBuf]) -> Vec<String> {
        items
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn a_tree_of_placeholders_is_removed_down_to_the_root_folder() {
        let plan = plan(&library());

        assert_eq!(
            paths(&plan.remove_files),
            ["All Photos/2024/a.jpg", "All Photos/2024/b.jpg"]
        );
        assert_eq!(
            paths(&plan.remove_folders),
            ["All Photos/2024", "All Photos", "Albums", ""]
        );
        assert!(plan.kept.is_empty());
        assert!(plan.removes_root());
    }

    #[test]
    fn a_hydrated_pinned_original_is_still_a_placeholder_and_is_removed() {
        // Hydration and pinning change the platform's state flags, not the identity or the sync
        // state, so the plan cannot tell a hydrated or pinned original from an unhydrated one.
        let tree = root(vec![file("pinned.jpg", "")]);

        let plan = plan(&tree);

        assert_eq!(paths(&plan.remove_files), ["pinned.jpg"]);
        assert!(plan.removes_root());
    }

    #[test]
    fn a_user_file_survives_and_keeps_its_folder_and_every_folder_above_it() {
        let tree = root(vec![folder(
            "All Photos",
            "",
            vec![folder(
                "2024",
                "All Photos",
                vec![file("a.jpg", "All Photos/2024"), user_file("mine.txt")],
            )],
        )]);

        let plan = plan(&tree);

        assert_eq!(paths(&plan.remove_files), ["All Photos/2024/a.jpg"]);
        assert!(plan.remove_folders.is_empty(), "{:?}", plan.remove_folders);
        assert!(!plan.removes_root());
        assert_eq!(plan.kept.len(), 1);
        assert_eq!(plan.kept[0].path, PathBuf::from("All Photos/2024/mine.txt"));
        assert_eq!(plan.kept[0].reason, KeptReason::NotCreatedByLymic);
    }

    #[test]
    fn a_folder_the_user_made_is_kept_even_when_it_is_empty() {
        let tree = root(vec![
            Node {
                name: "My edits".into(),
                kind: Kind::Directory,
                observed: Observed::Plain,
                children: Children::Listed(vec![]),
            },
            folder("Albums", "", vec![]),
        ]);

        let plan = plan(&tree);

        assert_eq!(paths(&plan.remove_folders), ["Albums"]);
        assert_eq!(plan.kept[0].path, PathBuf::from("My edits"));
        assert!(!plan.removes_root());
    }

    #[test]
    fn a_placeholder_inside_a_folder_the_user_made_is_still_removed() {
        let tree = root(vec![Node {
            name: "My edits".into(),
            kind: Kind::Directory,
            observed: Observed::Plain,
            children: Children::Listed(vec![Node {
                name: "x.jpg".into(),
                kind: Kind::File,
                observed: placeholder("My edits/x.jpg"),
                children: Children::None,
            }]),
        }]);

        let plan = plan(&tree);

        assert_eq!(paths(&plan.remove_files), ["My edits/x.jpg"]);
        assert!(plan.remove_folders.is_empty());
    }

    #[test]
    fn a_placeholder_the_user_changed_locally_is_their_file() {
        let mut edited = file("edited.jpg", "");
        edited.observed = Observed::Placeholder {
            identity: b"edited.jpg".to_vec(),
            in_sync: false,
        };

        let plan = plan(&root(vec![edited, file("fine.jpg", "")]));

        assert_eq!(paths(&plan.remove_files), ["fine.jpg"]);
        assert_eq!(plan.kept[0].reason, KeptReason::ChangedLocally);
        assert!(!plan.removes_root());
    }

    #[test]
    fn a_placeholder_that_is_not_where_lymic_put_it_is_kept() {
        let mut moved = file("moved.jpg", "");
        moved.observed = placeholder("All Photos/2024/moved.jpg");

        let plan = plan(&root(vec![moved]));

        assert!(plan.remove_files.is_empty());
        assert_eq!(plan.kept[0].reason, KeptReason::NotAtItsPlace);
    }

    #[test]
    fn a_name_that_is_not_unicode_cannot_match_its_identity_and_is_kept() {
        // On Windows a name is UTF-16 and may hold an unpaired surrogate.
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x61, 0xD800])
        };
        #[cfg(not(windows))]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0x61, 0xFF])
        };
        let odd = Node {
            name,
            kind: Kind::File,
            observed: placeholder("a"),
            children: Children::None,
        };

        let plan = plan(&root(vec![odd]));

        assert!(plan.remove_files.is_empty());
        assert_eq!(plan.kept[0].reason, KeptReason::NotAtItsPlace);
    }

    #[test]
    fn a_directory_that_cannot_be_listed_is_tried_for_removal_without_deleting_inside_it() {
        let tree = root(vec![Node {
            name: "All Photos".into(),
            kind: Kind::Directory,
            observed: placeholder("All Photos"),
            children: Children::Unreadable("provider not running".to_string()),
        }]);

        let plan = plan(&tree);

        assert!(plan.remove_files.is_empty());
        assert_eq!(paths(&plan.remove_folders), ["All Photos", ""]);
    }

    // ---------------------------------------------------------------- executing

    /// A fake filesystem over a set of paths that records the order of deletions and can be told
    /// to fail a path.
    struct FakeFs {
        present: RefCell<BTreeSet<PathBuf>>,
        fail: BTreeSet<PathBuf>,
        log: RefCell<Vec<String>>,
    }

    impl FakeFs {
        fn new(items: &[&str]) -> Self {
            Self {
                present: RefCell::new(items.iter().map(PathBuf::from).collect()),
                fail: BTreeSet::new(),
                log: RefCell::new(Vec::new()),
            }
        }

        fn failing(mut self, item: &str) -> Self {
            self.fail.insert(PathBuf::from(item));
            self
        }

        fn remove(&self, kind: &str, path: &Path) -> io::Result<()> {
            self.log
                .borrow_mut()
                .push(format!("{kind} {}", path.display()));
            if self.fail.contains(path) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "access denied",
                ));
            }
            if kind == "dir"
                && self
                    .present
                    .borrow()
                    .iter()
                    .any(|other| other != path && other.starts_with(path))
            {
                return Err(io::Error::from(io::ErrorKind::DirectoryNotEmpty));
            }
            if self.present.borrow_mut().remove(path) {
                Ok(())
            } else {
                Err(io::Error::from(io::ErrorKind::NotFound))
            }
        }
    }

    impl CleanupFs for FakeFs {
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.remove("file", path)
        }

        fn remove_folder(&self, path: &Path) -> io::Result<()> {
            self.remove("dir", path)
        }
    }

    fn root_path() -> PathBuf {
        PathBuf::from("Lib")
    }

    fn in_root(item: &str) -> String {
        root_path().join(item).to_string_lossy().into_owned()
    }

    fn all_items() -> Vec<String> {
        [
            "",
            "All Photos",
            "All Photos/2024",
            "All Photos/2024/a.jpg",
            "All Photos/2024/b.jpg",
            "Albums",
        ]
        .iter()
        .map(|item| in_root(item))
        .collect()
    }

    #[test]
    fn executing_removes_files_before_their_folders_and_reports_the_counts() {
        let items = all_items();
        let fs = FakeFs::new(&items.iter().map(String::as_str).collect::<Vec<_>>());

        let report = execute(&root_path(), plan(&library()), &fs);

        assert_eq!(report.removed_files, 2);
        assert_eq!(report.removed_folders, 4);
        assert!(!report.root_kept);
        assert_eq!(report.failed_count, 0);
        assert!(!report.needs_notice());
        assert!(fs.present.borrow().is_empty());
        let log = fs.log.borrow();
        let last_file = log
            .iter()
            .rposition(|entry| entry.starts_with("file"))
            .unwrap();
        let first_dir = log
            .iter()
            .position(|entry| entry.starts_with("dir"))
            .unwrap();
        assert!(last_file < first_dir, "{log:?}");
    }

    #[test]
    fn a_user_file_is_never_handed_to_the_filesystem_and_the_notice_says_so() {
        let tree = root(vec![file("a.jpg", ""), user_file("mine.txt")]);
        let fs = FakeFs::new(&[&in_root(""), &in_root("a.jpg"), &in_root("mine.txt")]);

        let report = execute(&root_path(), plan(&tree), &fs);

        assert!(fs.present.borrow().contains(&root_path().join("mine.txt")));
        assert!(fs.present.borrow().contains(&root_path()));
        assert!(
            fs.log
                .borrow()
                .iter()
                .all(|entry| !entry.contains("mine.txt")),
            "{:?}",
            fs.log.borrow()
        );
        assert_eq!(report.removed_files, 1);
        assert_eq!(report.kept_count, 1);
        assert_eq!(report.kept_examples, ["mine.txt"]);
        assert!(report.root_kept);
        assert!(report.needs_notice());
    }

    #[test]
    fn a_failure_half_way_is_reported_and_the_rest_still_goes_on() {
        let items = all_items();
        let fs = FakeFs::new(&items.iter().map(String::as_str).collect::<Vec<_>>())
            .failing(&in_root("All Photos/2024/a.jpg"));

        let report = execute(&root_path(), plan(&library()), &fs);

        assert_eq!(report.removed_files, 1, "b.jpg is removed");
        assert_eq!(report.failed_count, 1);
        assert!(
            report.failed_examples[0].starts_with("All Photos"),
            "{:?}",
            report.failed_examples
        );
        assert!(report.failed_examples[0].contains("access denied"));
        assert!(report.root_kept);
        assert!(report.needs_notice());
        // `Albums` is unrelated to the failure and is removed; the folders above it are not tried.
        assert!(!fs.present.borrow().contains(&root_path().join("Albums")));
        assert!(
            fs.log
                .borrow()
                .iter()
                .all(|entry| entry != &format!("dir {}", in_root("All Photos/2024"))),
            "{:?}",
            fs.log.borrow()
        );
    }

    #[test]
    fn a_second_run_after_a_complete_one_finds_nothing_and_changes_nothing() {
        let items = all_items();
        let fs = FakeFs::new(&items.iter().map(String::as_str).collect::<Vec<_>>());
        execute(&root_path(), plan(&library()), &fs);

        // The root is gone, so a second look finds nothing; and replaying the first plan finds
        // everything gone, which counts as done.
        let replay = execute(&root_path(), plan(&library()), &fs);

        assert_eq!(replay.removed_files + replay.removed_folders, 0);
        assert_eq!(replay.failed_count, 0);
        assert!(!replay.needs_notice());
    }

    #[test]
    fn a_folder_that_gained_a_file_after_the_look_is_kept_not_emptied() {
        let tree = root(vec![folder(
            "All Photos",
            "",
            vec![file("a.jpg", "All Photos")],
        )]);
        // A user file appeared after the scan: it is not in the plan, so only the folder delete runs
        // and the filesystem refuses it.
        let fs = FakeFs::new(&[
            &in_root(""),
            &in_root("All Photos"),
            &in_root("All Photos/a.jpg"),
            &in_root("All Photos/new.txt"),
        ]);

        let report = execute(&root_path(), plan(&tree), &fs);

        assert!(fs
            .present
            .borrow()
            .contains(&root_path().join("All Photos/new.txt")));
        assert_eq!(report.removed_files, 1);
        assert_eq!(report.failed_count, 0);
        assert!(report.kept_examples.contains(&"All Photos".to_string()));
        assert!(report.root_kept);
    }

    // ---------------------------------------------------------------- the whole cleanup

    struct FakeHost {
        fs: FakeFs,
        tree: Option<Node>,
        scan_error: Option<String>,
        unregister_error: Option<String>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl FakeHost {
        fn new(tree: Option<Node>, fs: FakeFs) -> Self {
            Self {
                fs,
                tree,
                scan_error: None,
                unregister_error: None,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl CleanupFs for FakeHost {
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.calls.borrow_mut().push("delete");
            self.fs.remove_file(path)
        }

        fn remove_folder(&self, path: &Path) -> io::Result<()> {
            self.calls.borrow_mut().push("delete");
            self.fs.remove_folder(path)
        }
    }

    impl CleanupHost for FakeHost {
        fn disconnect(&self) {
            self.calls.borrow_mut().push("disconnect");
        }

        fn scan(&self, _root: &Path) -> Result<Option<Node>, String> {
            self.calls.borrow_mut().push("scan");
            match &self.scan_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.tree.clone()),
            }
        }

        fn unregister(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("unregister");
            match &self.unregister_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    fn deduplicated(calls: &[&'static str]) -> Vec<&'static str> {
        let mut out = calls.to_vec();
        out.dedup();
        out
    }

    #[test]
    fn the_cleanup_stops_serving_looks_at_the_folder_unregisters_and_only_then_deletes() {
        let items = all_items();
        let host = FakeHost::new(
            Some(library()),
            FakeFs::new(&items.iter().map(String::as_str).collect::<Vec<_>>()),
        );

        let report = clean_up(&root_path(), &host).unwrap();

        assert_eq!(
            deduplicated(&host.calls.borrow()),
            ["disconnect", "scan", "unregister", "delete"]
        );
        assert_eq!(report.removed_files, 2);
        assert!(!report.root_kept);
    }

    #[test]
    fn a_registration_that_cannot_be_removed_stops_the_cleanup_before_anything_is_deleted() {
        let mut host = FakeHost::new(Some(library()), FakeFs::new(&[]));
        host.unregister_error = Some("0x80070005".to_string());

        let error = clean_up(&root_path(), &host).unwrap_err();

        assert_eq!(error, CleanupError::Unregister("0x80070005".to_string()));
        assert!(error.user_message().starts_with(CLEANUP_FAILED_ERROR));
        assert!(!host.calls.borrow().contains(&"delete"));
    }

    #[test]
    fn a_folder_that_cannot_be_examined_changes_nothing() {
        let mut host = FakeHost::new(None, FakeFs::new(&[]));
        host.scan_error = Some("access denied".to_string());

        let error = clean_up(&root_path(), &host).unwrap_err();

        assert_eq!(error, CleanupError::Scan("access denied".to_string()));
        assert_eq!(*host.calls.borrow(), ["disconnect", "scan"]);
    }

    #[test]
    fn a_root_folder_that_no_longer_exists_still_gets_its_registration_removed() {
        let host = FakeHost::new(None, FakeFs::new(&[]));

        let report = clean_up(&root_path(), &host).unwrap();

        assert_eq!(*host.calls.borrow(), ["disconnect", "scan", "unregister"]);
        assert!(!report.needs_notice());
    }

    #[test]
    fn the_report_names_only_the_first_few_kept_items_and_counts_all() {
        let many: Vec<Node> = (0..8).map(|n| user_file(&format!("u{n}.txt"))).collect();

        let report = execute(&root_path(), plan(&root(many)), &FakeFs::new(&[]));

        assert_eq!(report.kept_count, 8);
        assert_eq!(report.kept_examples.len(), EXAMPLE_LIMIT);
    }

    // ---------------------------------------------------------------- the real filesystem

    #[test]
    fn the_real_filesystem_removes_files_and_empty_folders_and_never_recurses() {
        let base = std::env::temp_dir().join(format!("lymic-vl-cleanup-{}", uuid::Uuid::new_v4()));
        let root = base.join("Lib");
        std::fs::create_dir_all(root.join("All Photos")).unwrap();
        std::fs::write(root.join("All Photos").join("a.jpg"), b"x").unwrap();
        std::fs::write(root.join("mine.txt"), b"mine").unwrap();
        std::fs::create_dir_all(root.join("Albums").join("Holiday")).unwrap();
        std::fs::write(root.join("Albums").join("Holiday").join("keep.txt"), b"k").unwrap();
        let tree = Node {
            name: OsString::new(),
            kind: Kind::Directory,
            observed: Observed::Plain,
            children: Children::Listed(vec![
                folder("All Photos", "", vec![file("a.jpg", "All Photos")]),
                // The scan found this folder as a placeholder with an empty listing, but a user
                // file has appeared in it since; removal must not take it along.
                folder("Albums", "", vec![folder("Holiday", "Albums", vec![])]),
            ]),
        };

        let report = execute(&root, plan(&tree), &StdFs);

        let kept_file = std::fs::read(root.join("Albums").join("Holiday").join("keep.txt"));
        let user_file = std::fs::read(root.join("mine.txt"));
        let photos_gone = !root.join("All Photos").exists();
        let root_exists = root.exists();
        std::fs::remove_dir_all(&base).unwrap();

        assert_eq!(kept_file.unwrap(), b"k");
        assert_eq!(user_file.unwrap(), b"mine");
        assert!(photos_gone);
        assert!(root_exists);
        assert_eq!(report.removed_files, 1);
        assert!(report.root_kept);
    }
}
