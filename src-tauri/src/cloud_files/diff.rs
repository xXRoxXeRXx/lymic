//! Reconciliation of one directory: what to create, update and remove so the placeholders present
//! locally match what the projection lists now (ADR-0003, #280).
//!
//! Pure and platform-neutral: it compares the desired placeholders (`placeholder::plan`) with a
//! snapshot of the directory on disk and never touches either. The Windows adapter applies the
//! result.
//!
//! Safety rules, all decided here so they are tested on every platform:
//!
//! * Only a placeholder Lymic created is ever removed or updated. Lymic's placeholders carry their
//!   root-relative path as file identity (`identity.rs`), so a local item counts as Lymic's only when
//!   it is a placeholder whose identity is exactly the path it sits at. A file or folder somebody
//!   put there, and a placeholder with any other identity, is left alone.
//! * A placeholder that is not in sync (a hydrated file the user changed) is never removed and never
//!   updated.
//! * A name the projection skipped this time (no size known yet) is not "removed remotely": what is
//!   there stays until a later listing shows it.
//! * A user's file occupying the name of a new asset blocks that one asset; the file stays.

use super::identity;
use super::placeholder::PlaceholderSpec;
use std::collections::{HashMap, HashSet};

/// What the platform says about one item in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalState {
    /// A Cloud Files placeholder.
    Placeholder {
        /// The file identity, decoded as a path; `None` when it is not UTF-8 or absent.
        identity: Option<String>,
        /// False once the user changed the content (a hydrated and modified file).
        in_sync: bool,
    },
    /// Not a placeholder: a file or folder that is not Lymic's.
    Foreign,
}

/// One item present in a directory on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalEntry {
    pub name: String,
    pub is_directory: bool,
    pub state: LocalState,
    /// The logical size (the size of the original for a placeholder); 0 for directories.
    pub size: u64,
    /// FILETIME ticks, as `PlaceholderSpec` carries them.
    pub created: i64,
    pub modified: i64,
}

/// A placeholder whose metadata no longer matches Immich.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Update {
    pub spec: PlaceholderSpec,
    /// The size changed, so the original is a different file: a hydrated copy must be dropped, not
    /// just relabelled.
    pub content_changed: bool,
}

/// A placeholder to delete: its Remote Asset (or album, or month) is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Removal {
    pub name: String,
    pub is_directory: bool,
}

/// What to do in one directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Plan {
    pub create: Vec<PlaceholderSpec>,
    pub update: Vec<Update>,
    pub remove: Vec<Removal>,
    /// Names left alone because something that is not Lymic's, or a placeholder the user modified,
    /// is in the way.
    pub kept: Vec<String>,
}

impl Plan {
    pub(super) fn is_empty(&self) -> bool {
        self.create.is_empty() && self.update.is_empty() && self.remove.is_empty()
    }
}

/// Names compare the way NTFS does by default, without regard to case (the projection guarantees
/// they are unique that way).
fn key(name: &str) -> String {
    name.to_lowercase()
}

/// The plan that makes the directory at `parent` (`""` is the root) match `desired`.
///
/// `skipped` are the names the projection could not describe this time; anything local with such a
/// name is kept untouched.
pub(super) fn diff(
    parent: &str,
    desired: &[PlaceholderSpec],
    skipped: &[String],
    local: &[LocalEntry],
) -> Plan {
    let local_by_name: HashMap<String, &LocalEntry> = local
        .iter()
        .map(|entry| (key(&entry.name), entry))
        .collect();
    let desired_names: HashSet<String> = desired.iter().map(|spec| key(&spec.name)).collect();
    let skipped_names: HashSet<String> = skipped.iter().map(|name| key(name)).collect();
    let mut plan = Plan::default();

    for spec in desired {
        let Some(entry) = local_by_name.get(&key(&spec.name)) else {
            plan.create.push(spec.clone());
            continue;
        };
        match &entry.state {
            LocalState::Placeholder {
                identity: Some(held),
                in_sync,
            } if entry.is_directory == spec.is_directory && held.as_bytes() == spec.identity => {
                if spec.is_directory || !changed(spec, entry) {
                    continue;
                }
                if *in_sync {
                    plan.update.push(Update {
                        spec: spec.clone(),
                        content_changed: spec.size != entry.size,
                    });
                } else {
                    plan.kept.push(spec.name.clone());
                }
            }
            _ => plan.kept.push(spec.name.clone()),
        }
    }

    for entry in local {
        let name = key(&entry.name);
        if desired_names.contains(&name) || skipped_names.contains(&name) {
            continue;
        }
        let ours = matches!(
            &entry.state,
            LocalState::Placeholder { identity: Some(held), in_sync: true }
                if *held == identity::child_path(parent, &entry.name)
        );
        if ours {
            plan.remove.push(Removal {
                name: entry.name.clone(),
                is_directory: entry.is_directory,
            });
        } else {
            plan.kept.push(entry.name.clone());
        }
    }
    plan
}

/// Whether the placeholder's size or times differ from what Immich says now. A time Immich did not
/// give (0) says nothing and never counts as a difference.
fn changed(spec: &PlaceholderSpec, entry: &LocalEntry) -> bool {
    spec.size != entry.size
        || (spec.modified != 0 && spec.modified != entry.modified)
        || (spec.created != 0 && spec.created != entry.created)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(parent: &str, name: &str, size: u64) -> PlaceholderSpec {
        PlaceholderSpec {
            name: name.to_string(),
            identity: identity::child_path(parent, name).into_bytes(),
            is_directory: false,
            size,
            created: 100,
            modified: 200,
        }
    }

    fn dir_spec(parent: &str, name: &str) -> PlaceholderSpec {
        PlaceholderSpec {
            is_directory: true,
            size: 0,
            created: 0,
            modified: 0,
            ..spec(parent, name, 0)
        }
    }

    /// The local counterpart of `spec`, as Lymic would have created it.
    fn local(parent: &str, name: &str, size: u64) -> LocalEntry {
        LocalEntry {
            name: name.to_string(),
            is_directory: false,
            state: LocalState::Placeholder {
                identity: Some(identity::child_path(parent, name)),
                in_sync: true,
            },
            size,
            created: 100,
            modified: 200,
        }
    }

    fn local_dir(parent: &str, name: &str) -> LocalEntry {
        LocalEntry {
            is_directory: true,
            size: 0,
            created: 0,
            modified: 0,
            ..local(parent, name, 0)
        }
    }

    fn foreign(name: &str) -> LocalEntry {
        LocalEntry {
            name: name.to_string(),
            is_directory: false,
            state: LocalState::Foreign,
            size: 5,
            created: 1,
            modified: 1,
        }
    }

    const DIR: &str = "Albums/Trip";

    #[test]
    fn a_remote_asset_without_a_placeholder_is_created() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 10), spec(DIR, "new.jpg", 20)],
            &[],
            &[local(DIR, "a.jpg", 10)],
        );

        assert_eq!(plan.create, vec![spec(DIR, "new.jpg", 20)]);
        assert!(plan.update.is_empty() && plan.remove.is_empty() && plan.kept.is_empty());
    }

    #[test]
    fn a_placeholder_whose_remote_asset_disappeared_is_removed() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 10)],
            &[],
            &[local(DIR, "a.jpg", 10), local(DIR, "gone.jpg", 7)],
        );

        assert_eq!(
            plan.remove,
            vec![Removal {
                name: "gone.jpg".to_string(),
                is_directory: false
            }]
        );
        assert!(plan.create.is_empty() && plan.update.is_empty());
    }

    #[test]
    fn a_directory_placeholder_that_disappeared_is_removed_as_a_directory() {
        let plan = diff(
            "",
            &[dir_spec("", "Albums")],
            &[],
            &[local_dir("", "Albums"), local_dir("", "Old Album")],
        );

        assert_eq!(
            plan.remove,
            vec![Removal {
                name: "Old Album".to_string(),
                is_directory: true
            }]
        );
    }

    #[test]
    fn a_changed_size_updates_the_placeholder_and_marks_the_content_as_changed() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 99)],
            &[],
            &[local(DIR, "a.jpg", 10)],
        );

        assert_eq!(
            plan.update,
            vec![Update {
                spec: spec(DIR, "a.jpg", 99),
                content_changed: true
            }]
        );
    }

    #[test]
    fn changed_times_alone_update_the_metadata_without_touching_the_content() {
        let mut desired = spec(DIR, "a.jpg", 10);
        desired.modified = 999;

        let plan = diff(DIR, &[desired.clone()], &[], &[local(DIR, "a.jpg", 10)]);

        assert_eq!(
            plan.update,
            vec![Update {
                spec: desired,
                content_changed: false
            }]
        );
    }

    #[test]
    fn a_time_immich_did_not_report_is_not_a_difference() {
        let mut desired = spec(DIR, "a.jpg", 10);
        desired.created = 0;
        desired.modified = 0;

        assert!(diff(DIR, &[desired], &[], &[local(DIR, "a.jpg", 10)]).is_empty());
    }

    #[test]
    fn an_unchanged_directory_listing_plans_nothing() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 10), spec(DIR, "b.jpg", 11)],
            &[],
            &[local(DIR, "b.jpg", 11), local(DIR, "a.jpg", 10)],
        );

        assert!(plan.is_empty() && plan.kept.is_empty());
    }

    #[test]
    fn a_placeholder_the_user_modified_is_neither_removed_nor_updated() {
        let mut modified = local(DIR, "edited.jpg", 10);
        modified.state = LocalState::Placeholder {
            identity: Some(identity::child_path(DIR, "edited.jpg")),
            in_sync: false,
        };
        let mut also_modified = local(DIR, "also.jpg", 10);
        also_modified.state = modified.state.clone();

        let plan = diff(
            DIR,
            &[spec(DIR, "also.jpg", 99)],
            &[],
            &[modified, also_modified],
        );

        assert!(plan.remove.is_empty() && plan.update.is_empty());
        assert_eq!(
            plan.kept,
            vec!["also.jpg".to_string(), "edited.jpg".to_string()]
        );
    }

    #[test]
    fn files_that_are_not_placeholders_are_never_touched() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 10)],
            &[],
            &[foreign("notes.txt"), local(DIR, "a.jpg", 10)],
        );

        assert!(plan.is_empty());
        assert_eq!(plan.kept, vec!["notes.txt".to_string()]);
    }

    #[test]
    fn a_placeholder_with_somebody_elses_identity_is_never_removed() {
        for identity in [
            Some("Albums/Other/x.jpg".to_string()),
            Some(String::new()),
            None,
        ] {
            let mut entry = local(DIR, "x.jpg", 1);
            entry.state = LocalState::Placeholder {
                identity,
                in_sync: true,
            };

            let plan = diff(DIR, &[], &[], &[entry]);

            assert!(plan.remove.is_empty());
            assert_eq!(plan.kept, vec!["x.jpg".to_string()]);
        }
    }

    #[test]
    fn a_users_file_in_the_way_of_a_new_asset_blocks_only_that_asset() {
        let plan = diff(
            DIR,
            &[spec(DIR, "IMG_1.jpg", 10), spec(DIR, "IMG_2.jpg", 20)],
            &[],
            &[foreign("img_1.JPG")],
        );

        assert_eq!(plan.create, vec![spec(DIR, "IMG_2.jpg", 20)]);
        assert_eq!(plan.kept, vec!["IMG_1.jpg".to_string()]);
    }

    #[test]
    fn a_placeholder_of_the_wrong_kind_is_kept_rather_than_replaced() {
        let plan = diff(DIR, &[dir_spec(DIR, "x")], &[], &[local(DIR, "x", 4)]);

        assert!(plan.is_empty());
        assert_eq!(plan.kept, vec!["x".to_string()]);
    }

    #[test]
    fn names_the_projection_skipped_stay_where_they_are() {
        let plan = diff(
            DIR,
            &[spec(DIR, "a.jpg", 10)],
            &["b.jpg".to_string()],
            &[local(DIR, "a.jpg", 10), local(DIR, "b.jpg", 3)],
        );

        assert!(plan.is_empty() && plan.kept.is_empty());
    }

    #[test]
    fn names_are_matched_without_regard_to_case() {
        let plan = diff(
            DIR,
            &[spec(DIR, "IMG.JPG", 10)],
            &[],
            &[local(DIR, "img.jpg", 10)],
        );

        // The local placeholder's identity is a different spelling, so it is not Lymic's current
        // placeholder for this name: it is kept and nothing is created beside it.
        assert!(plan.create.is_empty());
        assert!(plan.remove.is_empty());
    }

    #[test]
    fn a_listing_from_the_root_uses_the_bare_name_as_identity() {
        let plan = diff(
            "",
            &[dir_spec("", "Albums")],
            &[],
            &[local_dir("", "Albums"), local_dir("", "Gone")],
        );

        assert_eq!(plan.remove.len(), 1);
        assert_eq!(plan.remove[0].name, "Gone");
    }
}
