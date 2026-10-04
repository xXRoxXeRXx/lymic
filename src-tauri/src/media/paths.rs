use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize_folder_path(path: &Path) -> PathBuf {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    // Canonicalization resolves aliases for available folders. Offline folders
    // remain valid selections, so retain an absolute lexical path for them.
    let mut normalized_path = PathBuf::new();
    for component in absolute_path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized_path.pop();
            }
            component => normalized_path.push(component.as_os_str()),
        }
    }

    let canonical_path = std::fs::canonicalize(&normalized_path).unwrap_or(normalized_path);
    remove_windows_verbatim_prefix(canonical_path)
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

pub(crate) fn path_is_within(path: &Path, parent: &Path) -> bool {
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
        .map(|path| normalize_folder_path(&path))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.components().count());

    let mut folders = Vec::new();
    let mut overlaps = Vec::new();
    for path in paths {
        if let Some(parent) = folders
            .iter()
            .find(|folder: &&PathBuf| path_is_within(&path, folder))
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
        assert!(path_is_within(&child, &parent));
        assert!(path_is_within(&parent, &PathBuf::from(r"c:\FOTOS")));
    }
}
