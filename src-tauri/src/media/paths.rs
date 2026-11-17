use std::path::{Component, Path, PathBuf};

/// Normalizes a path without accessing the filesystem.
pub(crate) fn normalize_path_lexical(path: &Path) -> PathBuf {
    let mut normalized_path = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized_path.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized_path.pop();
                } else if !normalized_path.has_root() {
                    normalized_path.push(component.as_os_str());
                }
            }
            component => normalized_path.push(component.as_os_str()),
        }
    }

    normalize_windows_drive_letter(remove_windows_verbatim_prefix(normalized_path))
}

/// Resolves filesystem aliases for an available Watched Folder at its registration boundary.
///
/// Unavailable folders retain their absolute lexical path so they can be retried later.
pub(crate) fn canonicalize_folder_path(path: &Path) -> PathBuf {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let normalized_path = normalize_path_lexical(&absolute_path);
    let canonical_path = std::fs::canonicalize(&normalized_path).unwrap_or(normalized_path);
    normalize_path_lexical(&canonical_path)
}

#[cfg(windows)]
pub(crate) fn remove_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let path_string = path.to_string_lossy();
    if let Some(unc_path) = path_string.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", unc_path))
    } else if let Some(disk_path) = path_string.strip_prefix(r"\\?\") {
        if disk_path
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && disk_path.as_bytes().get(1) == Some(&b':')
        {
            PathBuf::from(disk_path)
        } else {
            path
        }
    } else {
        path
    }
}

#[cfg(not(windows))]
fn remove_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn normalize_windows_drive_letter(path: PathBuf) -> PathBuf {
    let path_string = path.to_string_lossy();
    let bytes = path_string.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let mut normalized = path_string.into_owned();
        normalized.replace_range(..1, &normalized[..1].to_ascii_uppercase());
        PathBuf::from(normalized)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn normalize_windows_drive_letter(path: PathBuf) -> PathBuf {
    path
}

fn path_component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str().to_string_lossy().to_lowercase()
            == right.as_os_str().to_string_lossy().to_lowercase()
    }

    #[cfg(not(windows))]
    {
        left == right
    }
}

fn with_normalized_pair<T>(left: &Path, right: &Path, f: impl FnOnce(&Path, &Path) -> T) -> T {
    let norm_left = normalize_path_lexical(left);
    let norm_right = normalize_path_lexical(right);
    f(&norm_left, &norm_right)
}

pub(crate) fn path_is_within_lexical(path: &Path, parent: &Path) -> bool {
    relative_path_within_lexical(path, parent).is_some()
}

pub(crate) fn path_is_within(path: &Path, parent: &Path) -> bool {
    with_normalized_pair(path, parent, path_is_within_lexical)
}

pub(crate) fn paths_equal_lexical(left: &Path, right: &Path) -> bool {
    let mut left_components = left.components();
    let mut right_components = right.components();
    let mut saw_any = false;
    loop {
        match (left_components.next(), right_components.next()) {
            (Some(l), Some(r)) => {
                saw_any = true;
                if !path_component_eq(l, r) {
                    return false;
                }
            }
            (None, None) => return saw_any,
            _ => return false,
        }
    }
}

pub(crate) fn paths_equal(left: &Path, right: &Path) -> bool {
    with_normalized_pair(left, right, paths_equal_lexical)
}

pub(crate) fn relative_path_within_lexical(
    norm_path: &Path,
    norm_parent: &Path,
) -> Option<PathBuf> {
    let mut parent_components = norm_parent.components().peekable();
    parent_components.peek()?;
    let mut path_components = norm_path.components();
    for parent_component in parent_components {
        let path_component = path_components.next()?;
        if !path_component_eq(path_component, parent_component) {
            return None;
        }
    }
    Some(path_components.collect())
}

/// Remaps paths inside an old Watched Folder to a new Watched Folder path
/// while preserving relative structure and target separator style.
pub(crate) struct PathRemapper {
    norm_old: PathBuf,
    pub(crate) clean_new: String,
}

impl PathRemapper {
    pub(crate) fn new(old_path: &str, new_path: &str) -> Self {
        Self {
            norm_old: normalize_path_lexical(Path::new(old_path)),
            clean_new: normalize_drive_letter_str(trim_trailing_separators_preserving_root(
                new_path,
            )),
        }
    }

    pub(crate) fn remap_if_changed(&self, candidate: &str) -> Option<String> {
        let norm_candidate = normalize_path_lexical(Path::new(candidate));
        let relative = relative_path_within_lexical(&norm_candidate, &self.norm_old)?;
        let remapped = if relative.as_os_str().is_empty() {
            self.clean_new.clone()
        } else {
            let sep = if self.clean_new.contains('/') && !self.clean_new.contains('\\') {
                "/"
            } else {
                std::path::MAIN_SEPARATOR_STR
            };
            let mut remapped = if self.clean_new == "/" {
                "/".to_owned()
            } else {
                self.clean_new.trim_end_matches(['/', '\\']).to_owned()
            };
            for component in relative.components() {
                if let Component::Normal(part) = component {
                    if !remapped.ends_with(['/', '\\']) {
                        remapped.push_str(sep);
                    }
                    remapped.push_str(&part.to_string_lossy());
                }
            }
            remapped
        };
        (remapped != candidate).then_some(remapped)
    }
}

fn trim_trailing_separators_preserving_root(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        return "/";
    }
    #[cfg(windows)]
    if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' && path.len() > 2 {
        return &path[..3];
    }
    trimmed
}

pub(crate) fn normalize_drive_letter_str(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("{}{}", (bytes[0] as char).to_ascii_uppercase(), &path[1..])
    } else {
        path.to_owned()
    }
}

pub(crate) fn overlapping_folder_paths(
    paths: Vec<PathBuf>,
) -> (Vec<PathBuf>, Vec<(PathBuf, PathBuf)>) {
    let mut paths = paths
        .into_iter()
        .map(|path| normalize_path_lexical(&path))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.components().count());

    let mut folders = Vec::new();
    let mut overlaps = Vec::new();
    for path in paths {
        if let Some(parent) = folders
            .iter()
            .find(|folder: &&PathBuf| path_is_within_lexical(&path, folder))
        {
            overlaps.push((path, parent.clone()));
        } else {
            folders.push(path);
        }
    }
    (folders, overlaps)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn relative_path_within(path: &Path, parent: &Path) -> Option<PathBuf> {
        with_normalized_pair(path, parent, relative_path_within_lexical)
    }

    #[test]
    fn removes_verbatim_prefix_and_compares_paths_case_insensitively() {
        let parent = remove_windows_verbatim_prefix(PathBuf::from(r"\\?\C:\Fotos"));
        let child = PathBuf::from(r"c:\fotos\Urlaub");
        assert_eq!(parent, PathBuf::from(r"C:\Fotos"));
        assert!(path_is_within_lexical(&child, &parent));
        assert!(path_is_within_lexical(&parent, &PathBuf::from(r"c:\FOTOS")));
    }

    #[test]
    fn lexical_normalization_removes_verbatim_prefix_resolves_segments_and_uppercases_drive() {
        assert_eq!(
            normalize_path_lexical(Path::new(r"\\?\c:\Fotos\.\Urlaub\..\Familie\\")),
            PathBuf::from(r"C:\Fotos\Familie")
        );
    }

    #[test]
    fn canonical_containment_and_equality_handle_windows_unicode_and_verbatim_prefixes() {
        assert!(paths_equal(
            Path::new(r"\\?\c:\FÖTOS\Sub\.."),
            Path::new(r"C:/fötos/")
        ));
        assert!(paths_equal(
            Path::new(r"\\?\UNC\Server\Share\Photos"),
            Path::new(r"\\server\share\photos\")
        ));
        assert_eq!(
            relative_path_within(
                Path::new(r"\\?\c:\Fötos\Screenshots\img.jpg"),
                Path::new(r"C:\FÖTOS"),
            ),
            Some(PathBuf::from(r"Screenshots\img.jpg"))
        );
        assert_eq!(
            relative_path_within(Path::new(r"C:\Photos"), Path::new(r"C:\Photos\")),
            Some(PathBuf::new())
        );
        assert_eq!(
            relative_path_within(Path::new(r"C:\Photos2\a.jpg"), Path::new(r"C:\Photos")),
            None
        );
        assert!(!path_is_within(
            Path::new(r"C:\Photos\a.jpg"),
            Path::new("")
        ));
        assert!(!paths_equal(Path::new(""), Path::new("")));
    }
}

#[cfg(test)]
mod lexical_tests {
    use super::*;

    #[test]
    fn lexical_normalization_resolves_posix_segments_without_touching_the_filesystem() {
        assert_eq!(
            normalize_path_lexical(Path::new("/media/photos/./2026/../2025/")),
            PathBuf::from("/media/photos/2025")
        );
    }

    #[test]
    fn lexical_containment_respects_component_boundaries() {
        assert!(path_is_within_lexical(
            Path::new("/media/photos/2025/image.jpg"),
            Path::new("/media/photos")
        ));
        assert!(!path_is_within_lexical(
            Path::new("/media/photos-backup/image.jpg"),
            Path::new("/media/photos")
        ));
    }

    #[test]
    fn lexical_normalization_preserves_leading_parent_segments() {
        assert_eq!(
            normalize_path_lexical(Path::new("../../photos/./2025")),
            PathBuf::from("../../photos/2025")
        );
    }

    #[test]
    fn empty_parent_never_contains_any_path() {
        assert!(!path_is_within(
            Path::new("/media/photos/a.jpg"),
            Path::new("")
        ));
        assert!(!paths_equal(Path::new(""), Path::new("")));
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_containment_and_equality_are_case_sensitive() {
        assert!(!path_is_within(
            Path::new("/media/photos/a.jpg"),
            Path::new("/Media/Photos")
        ));
        assert!(!paths_equal(
            Path::new("/Media/Photos"),
            Path::new("/media/photos")
        ));
        assert_eq!(
            with_normalized_pair(
                Path::new("/Media/Photos/Trips/a.jpg"),
                Path::new("/Media/Photos"),
                relative_path_within_lexical
            ),
            Some(PathBuf::from("Trips/a.jpg"))
        );
    }
}
