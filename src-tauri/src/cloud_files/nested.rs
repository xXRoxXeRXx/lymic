//! The rule that keeps the Virtual Library out of other sync roots (ADR-0003, gate 4 of #275).
//!
//! A sync root cannot be registered inside another one (Pictures redirected into OneDrive, for
//! example), and another cloud provider's root cannot lie inside it either. The check runs before
//! anything is created or registered, and it asks the operating system twice because neither
//! answer is complete on its own:
//!
//! 1. the Cloud Files platform: `CfGetSyncRootInfoByPath` on the folder and every existing parent
//!    succeeds for any path inside a registered root, whoever registered it;
//! 2. the registry list of registered roots (`SyncRootManager`), compared with the folder in both
//!    directions, which also finds a root that lies *inside* the folder.
//!
//! Both queries sit behind [`SyncRootProbe`] so the decision is testable on every platform.

use crate::media::paths::{normalize_path_lexical, path_is_within_lexical};
use std::path::{Path, PathBuf};

/// A sync root the operating system lists in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RegisteredRoot {
    /// The registration id (`<provider>!<SID>!<account>`).
    pub id: String,
    pub path: PathBuf,
}

/// What the operating system can tell about sync roots. Errors are plain messages: the check fails
/// closed on any of them instead of guessing that a folder is free.
pub(super) trait SyncRootProbe {
    /// The provider name of the sync root `path` lies in, or `None` outside every sync root.
    fn provider_at(&self, path: &Path) -> Result<Option<String>, String>;
    /// Every registered sync root.
    fn registered_roots(&self) -> Result<Vec<RegisteredRoot>, String>;
}

/// How the other sync root relates to the folder the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relation {
    /// The folder lies inside the other root.
    Inside,
    /// The other root lies inside the folder.
    Contains,
}

/// The other sync root that makes the folder unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NestedRoot {
    pub relation: Relation,
    pub root: PathBuf,
}

/// The folder as the operating system sees it, for the comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedTarget {
    /// The folder with every existing part resolved (junctions and symlinks, which is how a
    /// redirected Pictures folder shows up) and the part that does not exist yet appended.
    pub resolved: PathBuf,
    /// The deepest part that exists, resolved. The platform can only be asked about this one.
    pub existing: PathBuf,
}

/// Resolves `target` against the filesystem, even when it does not exist yet.
pub(super) fn resolve_target(target: &Path) -> ResolvedTarget {
    let target = normalize_path_lexical(target);
    for ancestor in target.ancestors() {
        if let Ok(canonical) = std::fs::canonicalize(ancestor) {
            let existing = normalize_path_lexical(&canonical);
            let tail = target.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return ResolvedTarget {
                resolved: normalize_path_lexical(&existing.join(tail)),
                existing,
            };
        }
    }
    ResolvedTarget {
        resolved: target.clone(),
        existing: target,
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_is_within_lexical(left, right) && path_is_within_lexical(right, left)
}

/// Finds the other sync root that nests with `target`, if any. A registration of ours at exactly
/// the same folder is a plain re-registration, never a conflict. `own_provider` is the provider
/// part of the registration ids Lymic creates.
pub(super) fn find_nested(
    target: &ResolvedTarget,
    own_provider: &str,
    probe: &impl SyncRootProbe,
) -> Result<Option<NestedRoot>, String> {
    let registry = probe.registered_roots()?;
    let is_ours = |root: &RegisteredRoot| root.id.starts_with(own_provider);
    // A root the registry attributes to us at exactly this folder.
    let own_here = registry
        .iter()
        .filter(|root| is_ours(root) && same_path(&root.path, &target.resolved))
        .map(|root| normalize_path_lexical(&root.path))
        .collect::<Vec<_>>();
    let cleared = |root: &Path| own_here.iter().any(|own| same_path(own, root));

    // The platform's view: the shallowest existing parent that is still inside a sync root is the
    // root itself.
    let mut enclosing = None;
    for ancestor in target.existing.ancestors() {
        if probe.provider_at(ancestor)?.is_some() {
            enclosing = Some(normalize_path_lexical(ancestor));
        }
    }
    if let Some(root) = enclosing.filter(|root| !cleared(root)) {
        return Ok(Some(NestedRoot {
            relation: Relation::Inside,
            root,
        }));
    }

    // The registry's view, in both directions.
    for root in &registry {
        let path = normalize_path_lexical(&root.path);
        if cleared(&path) {
            continue;
        }
        if path_is_within_lexical(&target.resolved, &path) {
            return Ok(Some(NestedRoot {
                relation: Relation::Inside,
                root: path,
            }));
        }
        if path_is_within_lexical(&path, &target.resolved) {
            return Ok(Some(NestedRoot {
                relation: Relation::Contains,
                root: path,
            }));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const OWN: &str = "Lymic.VirtualLibrary";

    fn base() -> PathBuf {
        std::env::temp_dir().join("lymic-nested-root-rules")
    }

    fn target(path: &Path) -> ResolvedTarget {
        ResolvedTarget {
            resolved: path.to_path_buf(),
            existing: path.to_path_buf(),
        }
    }

    /// An operating system with some sync roots: the platform answers for paths inside them and
    /// the registry lists them.
    #[derive(Default)]
    struct FakeOs {
        roots: Vec<(String, PathBuf)>,
        /// Registered roots the platform does not report (so only the registry knows them).
        registry_only: Vec<(String, PathBuf)>,
        failing_platform: bool,
        failing_registry: bool,
        asked: std::cell::RefCell<HashSet<PathBuf>>,
    }

    impl FakeOs {
        fn with_root(mut self, id: &str, path: PathBuf) -> Self {
            self.roots.push((id.to_string(), path));
            self
        }
    }

    impl SyncRootProbe for FakeOs {
        fn provider_at(&self, path: &Path) -> Result<Option<String>, String> {
            if self.failing_platform {
                return Err("CfGetSyncRootInfoByPath failed".to_string());
            }
            self.asked.borrow_mut().insert(path.to_path_buf());
            Ok(self
                .roots
                .iter()
                .find(|(_, root)| path_is_within_lexical(path, root))
                .map(|(id, _)| id.split('!').next().unwrap_or_default().to_string()))
        }

        fn registered_roots(&self) -> Result<Vec<RegisteredRoot>, String> {
            if self.failing_registry {
                return Err("registry unreadable".to_string());
            }
            Ok(self
                .roots
                .iter()
                .chain(&self.registry_only)
                .map(|(id, path)| RegisteredRoot {
                    id: id.clone(),
                    path: path.clone(),
                })
                .collect())
        }
    }

    #[test]
    fn a_folder_outside_every_sync_root_is_free() {
        let os = FakeOs::default().with_root("OneDrive!S-1!Personal", base().join("OneDrive"));

        let found = find_nested(&target(&base().join("Pictures").join("Lymic")), OWN, &os);

        assert_eq!(found, Ok(None));
    }

    #[test]
    fn a_folder_inside_onedrive_names_the_enclosing_root() {
        let onedrive = base().join("OneDrive");
        let os = FakeOs::default().with_root("OneDrive!S-1!Personal", onedrive.clone());

        let found = find_nested(&target(&onedrive.join("Pictures").join("Lymic")), OWN, &os);

        assert_eq!(
            found,
            Ok(Some(NestedRoot {
                relation: Relation::Inside,
                root: onedrive
            }))
        );
    }

    #[test]
    fn the_shallowest_enclosing_root_is_reported_when_roots_are_stacked() {
        let outer = base().join("Outer");
        let inner = outer.join("Inner");
        let os = FakeOs::default()
            .with_root("A!S-1!x", inner.clone())
            .with_root("B!S-1!x", outer.clone());

        let found = find_nested(&target(&inner.join("Lymic")), OWN, &os).unwrap();

        assert_eq!(found.unwrap().root, outer);
    }

    #[test]
    fn the_registry_catches_an_enclosing_root_the_platform_does_not_report() {
        let onedrive = base().join("OneDrive");
        let mut os = FakeOs::default();
        os.registry_only
            .push(("OneDrive!S-1!Personal".to_string(), onedrive.clone()));

        let found = find_nested(&target(&onedrive.join("Lymic")), OWN, &os);

        assert_eq!(
            found,
            Ok(Some(NestedRoot {
                relation: Relation::Inside,
                root: onedrive
            }))
        );
    }

    #[test]
    fn a_root_inside_the_chosen_folder_is_a_conflict_too() {
        let pictures = base().join("Pictures");
        let nested = pictures.join("Synced");
        let mut os = FakeOs::default();
        os.registry_only
            .push(("Dropbox!S-1!x".to_string(), nested.clone()));

        let found = find_nested(&target(&pictures), OWN, &os);

        assert_eq!(
            found,
            Ok(Some(NestedRoot {
                relation: Relation::Contains,
                root: nested
            }))
        );
    }

    #[test]
    fn a_sibling_whose_name_merely_starts_the_same_is_not_nested() {
        let mut os = FakeOs::default();
        os.registry_only
            .push(("X!S-1!x".to_string(), base().join("Lymic2")));

        assert_eq!(
            find_nested(&target(&base().join("Lymic")), OWN, &os),
            Ok(None)
        );
    }

    #[test]
    fn our_own_registration_of_exactly_this_folder_is_not_a_conflict() {
        let root = base().join("Pictures").join("Lymic");
        let os = FakeOs::default().with_root(&format!("{OWN}!S-1!Account"), root.clone());

        assert_eq!(find_nested(&target(&root), OWN, &os), Ok(None));
    }

    #[test]
    fn our_own_registration_of_a_parent_folder_still_blocks_a_folder_inside_it() {
        let parent = base().join("Pictures").join("Lymic");
        let os = FakeOs::default().with_root(&format!("{OWN}!S-1!Account"), parent.clone());

        let found = find_nested(&target(&parent.join("Sub")), OWN, &os);

        assert_eq!(
            found,
            Ok(Some(NestedRoot {
                relation: Relation::Inside,
                root: parent
            }))
        );
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_is_judged_by_its_deepest_existing_parent() {
        let onedrive = base().join("OneDrive");
        let os = FakeOs::default().with_root("OneDrive!S-1!Personal", onedrive.clone());
        let resolved = ResolvedTarget {
            resolved: onedrive.join("Not").join("Yet"),
            existing: onedrive.clone(),
        };

        let found = find_nested(&resolved, OWN, &os).unwrap();

        assert_eq!(found.unwrap().root, onedrive);
    }

    #[test]
    fn the_platform_is_asked_about_the_folder_and_every_parent() {
        let os = FakeOs::default();
        let deep = base().join("a").join("b");

        find_nested(&target(&deep), OWN, &os).unwrap();

        let asked = os.asked.borrow();
        assert!(asked.contains(&deep));
        assert!(asked.contains(&base().join("a")));
        assert!(asked.contains(&base()));
    }

    #[test]
    fn a_failing_platform_or_registry_query_fails_the_check_instead_of_guessing() {
        let root = target(&base().join("Lymic"));
        let platform = FakeOs {
            failing_platform: true,
            ..Default::default()
        };
        let registry = FakeOs {
            failing_registry: true,
            ..Default::default()
        };

        assert!(find_nested(&root, OWN, &platform).is_err());
        assert!(find_nested(&root, OWN, &registry).is_err());
    }

    #[test]
    fn resolving_a_missing_folder_keeps_the_part_that_does_not_exist() {
        let existing = std::env::temp_dir().join(format!("lymic-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&existing).unwrap();

        let resolved = resolve_target(&existing.join("Pictures").join("Lymic"));

        let canonical_existing = normalize_path_lexical(&std::fs::canonicalize(&existing).unwrap());
        assert_eq!(resolved.existing, canonical_existing);
        assert_eq!(
            resolved.resolved,
            canonical_existing.join("Pictures").join("Lymic")
        );
        std::fs::remove_dir_all(&existing).unwrap();
    }

    #[test]
    fn resolving_an_existing_folder_resolves_it_whole() {
        let existing = std::env::temp_dir().join(format!("lymic-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&existing).unwrap();

        let resolved = resolve_target(&existing);

        assert_eq!(resolved.resolved, resolved.existing);
        std::fs::remove_dir_all(&existing).unwrap();
    }
}
