//! Turning the Virtual Library on: the order of the steps and when to stop (ADR-0003).
//!
//! Nothing is created or registered until the folder has been checked against other sync roots. A
//! folder that nests with another root is refused with an error that asks for another folder; a
//! registration is never attempted in that case. The Windows calls themselves sit behind
//! [`SyncRootHost`], so this order is testable on every platform.

use super::nested::{self, NestedRoot, SyncRootProbe};
use std::path::Path;

/// Error text prefix when the folder lies inside, or contains, another sync root. The text after
/// `": "` is that other root's path. The frontend turns it into a localized request for another
/// folder.
pub(crate) const NESTED_SYNC_ROOT_ERROR: &str = "VIRTUAL_LIBRARY_NESTED_SYNC_ROOT";
/// Error text prefix for any other failure to start the Virtual Library; the text after `": "` is
/// the technical reason.
pub(crate) const START_FAILED_ERROR: &str = "VIRTUAL_LIBRARY_START_FAILED";

/// The part of the operating system the activation steps need.
pub(super) trait SyncRootHost: SyncRootProbe {
    fn create_root_folder(&self, root: &Path) -> Result<(), String>;
    /// Registers the sync root (progressive hydration, population per directory).
    fn register(&self, root: &Path) -> Result<(), String>;
    /// Starts serving callbacks for the registered root.
    fn connect(&self, root: &Path) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivationError {
    /// The folder nests with another sync root; nothing was created or registered.
    NestedSyncRoot(NestedRoot),
    /// The check itself could not run; nothing was created or registered.
    Check(String),
    Folder(String),
    Register(String),
    Connect(String),
    /// Not Windows.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

impl ActivationError {
    /// The error text the frontend receives. It never carries credentials.
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::NestedSyncRoot(nested) => {
                format!("{NESTED_SYNC_ROOT_ERROR}: {}", nested.root.display())
            }
            Self::Check(reason) => {
                format!("{START_FAILED_ERROR}: could not check for other sync folders: {reason}")
            }
            Self::Folder(reason) => {
                format!("{START_FAILED_ERROR}: could not create the folder: {reason}")
            }
            Self::Register(reason) => {
                format!("{START_FAILED_ERROR}: could not register the sync folder: {reason}")
            }
            Self::Connect(reason) => {
                format!("{START_FAILED_ERROR}: could not start serving the sync folder: {reason}")
            }
            Self::Unsupported => {
                format!("{START_FAILED_ERROR}: the Virtual Library needs Windows")
            }
        }
    }
}

/// Checks `root` against other sync roots, then creates it, registers it and starts serving it.
/// Running it again for a root Lymic already registered is a plain re-registration. Blocking: call
/// it from a blocking context.
pub(super) fn activate(root: &Path, host: &impl SyncRootHost) -> Result<(), ActivationError> {
    let target = nested::resolve_target(root);
    match nested::find_nested(&target, super::PROVIDER_ID, host) {
        Ok(Some(other)) => return Err(ActivationError::NestedSyncRoot(other)),
        Ok(None) => {}
        Err(reason) => return Err(ActivationError::Check(reason)),
    }
    host.create_root_folder(root)
        .map_err(ActivationError::Folder)?;
    host.register(root).map_err(ActivationError::Register)?;
    host.connect(root).map_err(ActivationError::Connect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_files::nested::{RegisteredRoot, Relation};
    use std::cell::RefCell;
    use std::path::PathBuf;

    struct FakeHost {
        calls: RefCell<Vec<&'static str>>,
        enclosing_root: Option<PathBuf>,
        probe_fails: bool,
        fail_at: Option<&'static str>,
    }

    impl FakeHost {
        fn free() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                enclosing_root: None,
                probe_fails: false,
                fail_at: None,
            }
        }

        fn step(&self, name: &'static str) -> Result<(), String> {
            self.calls.borrow_mut().push(name);
            if self.fail_at == Some(name) {
                Err(format!("{name} failed"))
            } else {
                Ok(())
            }
        }
    }

    impl SyncRootProbe for FakeHost {
        fn provider_at(&self, path: &Path) -> Result<Option<String>, String> {
            if self.probe_fails {
                return Err("platform query failed".to_string());
            }
            Ok(self
                .enclosing_root
                .as_ref()
                .filter(|root| crate::media::paths::path_is_within_lexical(path, root))
                .map(|_| "OneDrive".to_string()))
        }

        fn registered_roots(&self) -> Result<Vec<RegisteredRoot>, String> {
            Ok(Vec::new())
        }
    }

    impl SyncRootHost for FakeHost {
        fn create_root_folder(&self, _root: &Path) -> Result<(), String> {
            self.step("create")
        }

        fn register(&self, _root: &Path) -> Result<(), String> {
            self.step("register")
        }

        fn connect(&self, _root: &Path) -> Result<(), String> {
            self.step("connect")
        }
    }

    /// Resolved the way the check resolves a folder, so the fake sync roots below compare equal
    /// even where the temp directory is reached through a short name or a junction.
    fn base() -> PathBuf {
        crate::media::paths::normalize_path_lexical(
            &std::fs::canonicalize(std::env::temp_dir()).unwrap(),
        )
        .join("lymic-activation-tests")
    }

    fn root() -> PathBuf {
        base().join("Pictures").join("Lymic")
    }

    #[test]
    fn a_free_folder_is_created_registered_and_connected_in_that_order() {
        let host = FakeHost::free();

        assert_eq!(activate(&root(), &host), Ok(()));

        assert_eq!(*host.calls.borrow(), vec!["create", "register", "connect"]);
    }

    #[test]
    fn a_folder_inside_another_sync_root_is_refused_and_nothing_is_created_or_registered() {
        // A sync root exists on disk: the platform is only asked about folders that do.
        let onedrive = base();
        std::fs::create_dir_all(&onedrive).unwrap();
        let host = FakeHost {
            enclosing_root: Some(onedrive.clone()),
            ..FakeHost::free()
        };

        let error = activate(&root(), &host).unwrap_err();
        std::fs::remove_dir_all(&onedrive).unwrap();

        assert_eq!(
            error,
            ActivationError::NestedSyncRoot(NestedRoot {
                relation: Relation::Inside,
                root: onedrive.clone(),
            })
        );
        assert!(host.calls.borrow().is_empty(), "no step may run");
        assert_eq!(
            error.user_message(),
            format!("{NESTED_SYNC_ROOT_ERROR}: {}", onedrive.display())
        );
    }

    #[test]
    fn a_failing_check_refuses_instead_of_registering_blindly() {
        let host = FakeHost {
            probe_fails: true,
            ..FakeHost::free()
        };

        let error = activate(&root(), &host).unwrap_err();

        assert!(matches!(error, ActivationError::Check(_)), "{error:?}");
        assert!(host.calls.borrow().is_empty());
        assert!(error.user_message().starts_with(START_FAILED_ERROR));
    }

    #[test]
    fn each_failing_step_stops_the_ones_after_it() {
        for (failing, ran) in [
            ("create", vec!["create"]),
            ("register", vec!["create", "register"]),
            ("connect", vec!["create", "register", "connect"]),
        ] {
            let host = FakeHost {
                fail_at: Some(failing),
                ..FakeHost::free()
            };

            let error = activate(&root(), &host).unwrap_err();

            assert_eq!(*host.calls.borrow(), ran, "{failing}");
            let message = error.user_message();
            assert!(message.starts_with(START_FAILED_ERROR), "{message}");
            assert!(message.contains(&format!("{failing} failed")), "{message}");
        }
    }
}
