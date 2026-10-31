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

pub(crate) fn path_is_within_lexical(path: &Path, parent: &Path) -> bool {
    let mut path_components = path.components();
    parent
        .components()
        .all(|parent_component| match path_components.next() {
            Some(path_component) => path_component_eq(path_component, parent_component),
            None => false,
        })
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
}
