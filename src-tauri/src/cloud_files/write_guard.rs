//! The read-only rule of the Virtual Library (ADR-0003): what happens when the user deletes or
//! renames a Virtual Placeholder in Explorer.
//!
//! The Cloud Files platform asks the provider first (`NOTIFY_DELETE`, `NOTIFY_RENAME`) and the
//! provider's answer decides. Lymic's answer is always no: Explorer cannot tell evicting a local
//! copy from deleting the Remote Asset, and Lymic never writes to Immich from the Virtual Library.
//! Moving to the Recycle Bin and moving out of the root are renames to the platform, so they land
//! here too. What the user creates themselves in the folder is not a placeholder, raises no such
//! callback and is not covered by this module: Lymic ignores it and never uploads it.
//!
//! There is no "allow" answer on purpose, so the decision cannot be a boolean someone flips for a
//! case that looks harmless; what it returns is the reason, which the adapter logs.

/// What the platform asked about, copied from the callback's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation<'a> {
    /// `NOTIFY_DELETE`, also when it is an undelete.
    Delete,
    /// `NOTIFY_RENAME`: `target` is the new path as the platform reports it (volume relative, for
    /// example `\$Recycle.Bin\S-1-5-21-...\$R1ABCD.jpg`), `target_in_scope` whether that path is
    /// still inside the sync root.
    Rename {
        target: &'a str,
        target_in_scope: bool,
    },
}

/// Why the operation is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    Delete,
    RenameToRecycleBin,
    RenameOutOfRoot,
    RenameWithinRoot,
}

impl Refusal {
    /// A short phrase for the diagnostic log.
    pub(super) fn describe(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::RenameToRecycleBin => "rename into the Recycle Bin",
            Self::RenameOutOfRoot => "rename out of the Virtual Library folder",
            Self::RenameWithinRoot => "rename inside the Virtual Library folder",
        }
    }
}

/// The refusal for `operation`. Every operation is refused.
pub(super) fn refuse(operation: Operation<'_>) -> Refusal {
    match operation {
        Operation::Delete => Refusal::Delete,
        Operation::Rename {
            target,
            target_in_scope,
        } => {
            if is_in_recycle_bin(target) {
                Refusal::RenameToRecycleBin
            } else if target_in_scope {
                Refusal::RenameWithinRoot
            } else {
                Refusal::RenameOutOfRoot
            }
        }
    }
}

/// Whether `path` has the Recycle Bin folder as one of its components, compared without regard to
/// case, so a name that merely contains `$Recycle.Bin` is not mistaken for it.
fn is_in_recycle_bin(path: &str) -> bool {
    path.split(['\\', '/'])
        .any(|component| component.eq_ignore_ascii_case("$Recycle.Bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delete_is_refused() {
        assert_eq!(refuse(Operation::Delete), Refusal::Delete);
    }

    #[test]
    fn a_rename_inside_the_root_is_refused() {
        let rename = Operation::Rename {
            target: r"\Users\me\Pictures\Lymic\Albums\Holiday\renamed.jpg",
            target_in_scope: true,
        };

        assert_eq!(refuse(rename), Refusal::RenameWithinRoot);
    }

    #[test]
    fn a_rename_out_of_the_root_is_refused() {
        let rename = Operation::Rename {
            target: r"\Users\me\Documents\IMG_0001.jpg",
            target_in_scope: false,
        };

        assert_eq!(refuse(rename), Refusal::RenameOutOfRoot);
    }

    #[test]
    fn a_rename_into_the_recycle_bin_is_refused_whatever_the_case_of_its_name() {
        for target in [
            r"\$Recycle.Bin\S-1-5-21-1-2-3-1001\$R1ABCD.jpg",
            r"\$RECYCLE.BIN\S-1-5-21-1-2-3-1001\$R1ABCD.jpg",
            r"D:\$Recycle.Bin\S-1-5-21-1-2-3-1001\$R1ABCD.jpg",
            "/$recycle.bin/S-1-5-21-1-2-3-1001/$R1ABCD.jpg",
        ] {
            let rename = Operation::Rename {
                target,
                target_in_scope: false,
            };

            assert_eq!(refuse(rename), Refusal::RenameToRecycleBin, "{target}");
        }
    }

    #[test]
    fn a_name_that_only_looks_like_the_recycle_bin_is_an_ordinary_rename_out_of_the_root() {
        for target in [
            r"\Users\me\$Recycle.Bin.old\IMG_0001.jpg",
            r"\Users\me\My$Recycle.Bin\IMG_0001.jpg",
            r"\Users\me\IMG_0001.$Recycle.Bin",
        ] {
            let rename = Operation::Rename {
                target,
                target_in_scope: false,
            };

            assert_eq!(refuse(rename), Refusal::RenameOutOfRoot, "{target}");
        }
    }
}
