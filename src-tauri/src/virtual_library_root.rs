//! Where the Virtual Library lives on disk, and the rule that keeps Watched Folders away from it
//! (ADR-0003). Platform-neutral: the Windows Known Folder lookup sits behind [`KnownFolders`] so the
//! default root and the overlap rule are testable on every platform.

use crate::media::paths::{
    canonicalize_folder_path, normalize_path_lexical, path_is_within_lexical,
};
use std::path::{Path, PathBuf};

/// The name of the Virtual Library folder inside the Pictures folder.
const ROOT_FOLDER_NAME: &str = "Lymic";

/// Locations the operating system owns, resolved through its own API and never assembled from
/// environment variables, because users can redirect them (for example Pictures to another drive).
pub(crate) trait KnownFolders {
    fn pictures(&self) -> Option<PathBuf>;
}

/// The real operating system. Only Windows can resolve a Known Folder; elsewhere the Virtual
/// Library is unavailable and there is no default root.
pub(crate) struct SystemKnownFolders;

impl KnownFolders for SystemKnownFolders {
    fn pictures(&self) -> Option<PathBuf> {
        pictures_known_folder()
    }
}

#[cfg(windows)]
fn pictures_known_folder() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};

    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: `FOLDERID_Pictures` outlives the call; on success the shell allocates `raw` as a
    // NUL-terminated wide string that is freed below with `CoTaskMemFree`.
    let status =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut raw) };
    let path = if status >= 0 && !raw.is_null() {
        // SAFETY: `raw` points to a NUL-terminated buffer owned by the shell until freed below.
        let wide = unsafe {
            let mut length = 0;
            while *raw.add(length) != 0 {
                length += 1;
            }
            std::slice::from_raw_parts(raw, length)
        };
        Some(PathBuf::from(OsString::from_wide(wide)))
    } else {
        None
    };
    if !raw.is_null() {
        // SAFETY: `raw` was allocated by `SHGetKnownFolderPath`, which requires this release even
        // after a failure.
        unsafe { CoTaskMemFree(raw as *const _) };
    }
    path
}

#[cfg(not(windows))]
fn pictures_known_folder() -> Option<PathBuf> {
    None
}

/// The Pictures folder as the operating system reports it, plus `Lymic`. `None` when the system
/// cannot say where Pictures is.
pub(crate) fn default_root(known_folders: &impl KnownFolders) -> Option<PathBuf> {
    known_folders
        .pictures()
        .map(|pictures| pictures.join(ROOT_FOLDER_NAME))
}

/// The root the user chose, or else the default. A blank choice counts as no choice.
pub(crate) fn effective_root(
    chosen: Option<&str>,
    known_folders: &impl KnownFolders,
) -> Option<PathBuf> {
    match chosen.map(str::trim).filter(|chosen| !chosen.is_empty()) {
        Some(chosen) => Some(PathBuf::from(chosen)),
        None => default_root(known_folders),
    }
}

/// How a Watched Folder relates to the Virtual Library root when they overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootOverlap {
    /// The folder is the root itself.
    Equal,
    /// The folder lies inside the root.
    Inside,
    /// The folder is a parent of the root.
    Contains,
}

impl RootOverlap {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Equal => "is the Virtual Library folder",
            Self::Inside => "is inside the Virtual Library folder",
            Self::Contains => "contains the Virtual Library folder",
        }
    }
}

/// Compares two paths lexically; callers pass canonical paths when they have them.
pub(crate) fn overlap(root: &Path, folder: &Path) -> Option<RootOverlap> {
    let root = normalize_path_lexical(root);
    let folder = normalize_path_lexical(folder);
    match (
        path_is_within_lexical(&folder, &root),
        path_is_within_lexical(&root, &folder),
    ) {
        (true, true) => Some(RootOverlap::Equal),
        (true, false) => Some(RootOverlap::Inside),
        (false, true) => Some(RootOverlap::Contains),
        (false, false) => None,
    }
}

/// The existing Watched Folders that overlap `root`, with how. Touches the filesystem to resolve the
/// root's aliases, so call it from a blocking context.
pub(crate) fn overlapping_folders(root: &Path, folders: &[PathBuf]) -> Vec<(PathBuf, RootOverlap)> {
    let root = canonicalize_folder_path(root);
    folders
        .iter()
        .filter_map(|folder| overlap(&root, folder).map(|relation| (folder.clone(), relation)))
        .collect()
}

/// The refusal shown when a Watched Folder is registered on top of the Virtual Library.
pub(crate) fn registration_refusal(folder: &Path, root: &Path, overlap: RootOverlap) -> String {
    format!(
        "Folder '{}' {} '{}'. Synchronization must never scan the Virtual Library, so choose another folder or change the Virtual Library folder in settings.",
        folder.display(),
        overlap.describe(),
        root.display()
    )
}

/// A Known Folder lookup with a fixed answer, for tests on any platform.
#[cfg(test)]
pub(crate) struct FixedKnownFolders(pub(crate) Option<PathBuf>);

#[cfg(test)]
impl KnownFolders for FixedKnownFolders {
    fn pictures(&self) -> Option<PathBuf> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        std::env::temp_dir().join("lymic-root-rules")
    }

    #[test]
    fn the_default_root_is_lymic_inside_the_pictures_folder_the_system_reports() {
        // A redirected Pictures folder, nowhere near the profile directory.
        let redirected = base().join("OtherDrive").join("Fotos");
        let known = FixedKnownFolders(Some(redirected.clone()));

        assert_eq!(default_root(&known), Some(redirected.join("Lymic")));
    }

    #[test]
    fn there_is_no_default_root_when_the_system_cannot_resolve_pictures() {
        assert_eq!(default_root(&FixedKnownFolders(None)), None);
        assert_eq!(effective_root(None, &FixedKnownFolders(None)), None);
    }

    #[test]
    fn a_chosen_root_replaces_the_default_but_a_blank_choice_does_not() {
        let known = FixedKnownFolders(Some(base().join("Pictures")));
        let chosen = base().join("Elsewhere");

        assert_eq!(
            effective_root(Some(&chosen.to_string_lossy()), &known),
            Some(chosen)
        );
        assert_eq!(
            effective_root(Some("  "), &known),
            Some(base().join("Pictures").join("Lymic"))
        );
        assert_eq!(
            effective_root(None, &known),
            Some(base().join("Pictures").join("Lymic"))
        );
    }

    #[test]
    fn the_system_known_folder_lookup_resolves_pictures_only_on_windows() {
        let pictures = SystemKnownFolders.pictures();
        assert_eq!(pictures.is_some(), cfg!(windows));
        if let Some(pictures) = pictures {
            assert!(pictures.is_absolute(), "{}", pictures.display());
        }
    }

    #[test]
    fn classifies_how_a_folder_overlaps_the_root() {
        let root = base().join("Pictures").join("Lymic");

        assert_eq!(overlap(&root, &root), Some(RootOverlap::Equal));
        assert_eq!(
            overlap(&root, &root.join("Albums").join("Trip")),
            Some(RootOverlap::Inside)
        );
        assert_eq!(
            overlap(&root, &base().join("Pictures")),
            Some(RootOverlap::Contains)
        );
        assert_eq!(
            overlap(&root, &base().join("Pictures").join("Lymic2")),
            None
        );
        assert_eq!(overlap(&root, &base().join("Documents")), None);
    }
}
