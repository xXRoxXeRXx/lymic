use crate::db::WatchedFolder;
use crate::media::model::{upload_units_from_assets, Asset, UploadUnit};
use crate::media::paths::{normalize_path_lexical, relative_path_within_lexical};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

fn matches_segment(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            retry += 1;
            v = retry;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|character| *character == b'*')
}

fn matches_pattern(pattern: &str, relative_path: &Path) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let pattern_segments = pattern
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let path_segments = relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let path_segment_refs = path_segments.iter().map(String::as_str).collect::<Vec<_>>();

    fn matches(pattern: &[&str], path: &[&str]) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => {
                matches(rest, path) || (!path.is_empty() && matches(pattern, &path[1..]))
            }
            Some((segment, rest)) => path.first().is_some_and(|value| {
                matches_segment(segment.as_bytes(), value.as_bytes()) && matches(rest, &path[1..])
            }),
        }
    }

    matches(&pattern_segments, &path_segment_refs)
}

fn folder_for_path<'a>(
    path: &str,
    folders: &'a [WatchedFolder],
) -> Option<(&'a WatchedFolder, PathBuf)> {
    let norm_path = normalize_path_lexical(Path::new(path));
    folders
        .iter()
        .filter_map(|folder| {
            let norm_folder = normalize_path_lexical(Path::new(&folder.path));
            relative_path_within_lexical(&norm_path, &norm_folder).map(|relative| {
                let depth = norm_folder.components().count();
                (folder, relative, depth)
            })
        })
        .max_by_key(|(_, _, depth)| *depth)
        .map(|(folder, relative, _)| (folder, relative))
}

pub(crate) fn watched_folder_for_path<'a>(
    folders: &'a [WatchedFolder],
    path: &Path,
) -> Option<&'a WatchedFolder> {
    folder_for_path(&path.to_string_lossy(), folders).map(|(folder, _)| folder)
}

fn path_is_excluded(path: &str, folders: &[WatchedFolder]) -> bool {
    folder_for_path(path, folders).is_some_and(|(folder, relative)| {
        folder
            .exclusion_patterns
            .iter()
            .any(|pattern| matches_pattern(pattern, &relative))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    Admitted,
    Excluded,
}

pub(crate) fn decide(unit: &UploadUnit, folders: &[WatchedFolder]) -> Admission {
    decide_paths(unit.assets().map(|asset| asset.path.as_str()), folders)
}

pub(crate) fn decide_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    folders: &[WatchedFolder],
) -> Admission {
    if paths
        .into_iter()
        .any(|path| path_is_excluded(path, folders))
    {
        Admission::Excluded
    } else {
        Admission::Admitted
    }
}

pub(crate) fn admitted_assets(assets: Vec<Asset>, folders: &[WatchedFolder]) -> Vec<Asset> {
    let admitted_paths = upload_units_from_assets(assets.clone())
        .into_iter()
        .filter(|unit| decide(unit, folders) == Admission::Admitted)
        .flat_map(|unit| {
            unit.assets()
                .map(|asset| asset.path.clone())
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>();
    assets
        .into_iter()
        .filter(|asset| admitted_paths.contains(&asset.path))
        .collect()
}

pub(crate) fn normalize_patterns(patterns: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim().to_owned();
        if pattern.is_empty() {
            continue;
        }
        if pattern.contains('\\')
            || pattern.starts_with('/')
            || pattern.contains("..")
            || pattern.contains('?')
            || pattern.contains('[')
            || pattern.contains(']')
            || pattern.contains('{')
            || pattern.contains('}')
            || pattern.contains('!')
        {
            return Err(format!(
                "Invalid exclusion pattern '{pattern}'. Use relative paths with names, * and ** only."
            ));
        }
        if !normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&pattern))
        {
            normalized.push(pattern);
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::Asset;
    use std::path::Path;

    fn asset(path: &str) -> Asset {
        Asset {
            path: path.to_owned(),
            size: 1,
            mtime: 1,
            target_album_id: None,
            remote_id: None,
        }
    }

    fn unit(image: &str, live_photo_video: Option<&str>) -> UploadUnit {
        UploadUnit {
            image: asset(image),
            live_photo_video: live_photo_video.map(asset),
        }
    }

    fn folder(path: &str, patterns: &[&str]) -> WatchedFolder {
        WatchedFolder {
            id: 1,
            path: path.to_owned(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: patterns
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect(),
        }
    }

    #[test]
    fn decides_upload_unit_admission_from_watched_folder_patterns() {
        let folders = [folder("C:/Photos", &["Screenshots/**", "**/*.mov"])];
        let cases = [
            (unit("C:/Photos/Trips/keep.jpg", None), Admission::Admitted),
            (
                unit("C:/Photos/Screenshots/Nested/private.JPG", None),
                Admission::Excluded,
            ),
            (unit("C:/Photos/Trips/video.MOV", None), Admission::Excluded),
            (unit("C:/Elsewhere/private.jpg", None), Admission::Admitted),
            (
                unit(
                    "C:/Photos/Trips/IMG_1.HEIC",
                    Some("C:/Photos/Trips/IMG_1.MOV"),
                ),
                Admission::Excluded,
            ),
        ];

        for (unit, expected) in cases {
            assert_eq!(decide(&unit, &folders), expected);
        }
    }

    #[test]
    fn normalizes_and_rejects_invalid_patterns_before_admission() {
        assert_eq!(
            normalize_patterns(vec![
                " Screenshots/** ".to_owned(),
                "screenshots/**".to_owned(),
                "".to_owned(),
            ])
            .unwrap(),
            vec!["Screenshots/**"]
        );
        assert!(normalize_patterns(vec!["../private/**".to_owned()]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn resolves_the_most_specific_watched_folder_case_insensitively() {
        let root = folder("C:/Photos", &["**/*.mov"]);
        let mut private = folder("C:/Photos/Private", &["**/*.jpg"]);
        private.id = 2;
        let folders = [root, private];

        assert_eq!(
            watched_folder_for_path(&folders, Path::new("c:/PHOTOS/private/nested/keep.JPG"))
                .map(|folder| folder.id),
            Some(2)
        );
        assert!(watched_folder_for_path(&folders, Path::new("C:/Elsewhere/keep.jpg")).is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn resolves_watched_folder_case_sensitively_while_matching_exclusion_globs_case_insensitively()
    {
        let root = folder("/Media/Photos", &["Screenshots/**"]);
        let mut private = folder("/Media/Photos/Private", &["**/*.jpg"]);
        private.id = 2;
        let folders = [root, private];

        assert!(
            watched_folder_for_path(&folders, Path::new("/media/photos/private/keep.JPG"))
                .is_none()
        );
        assert_eq!(
            watched_folder_for_path(&folders, Path::new("/Media/Photos/Private/nested/keep.JPG"))
                .map(|folder| folder.id),
            Some(2)
        );
        assert_eq!(
            decide(
                &unit("/Media/Photos/screenshots/private.JPG", None),
                &folders
            ),
            Admission::Excluded
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_admission_matches_verbatim_unicode_and_dot_segments_consistently() {
        let folders = [folder(r"\\?\C:\FÖTOS", &["Screenshots/**"])];
        assert_eq!(
            decide(&unit(r"c:\fötos\screenshots\nested.jpg", None), &folders),
            Admission::Excluded
        );
        assert_eq!(
            decide(
                &unit(r"C:\FÖTOS\Screenshots\..\Keep\photo.jpg", None),
                &folders
            ),
            Admission::Admitted
        );
    }

    #[test]
    fn exclusion_glob_matching_preserves_ascii_only_case_folding() {
        let folders = [folder("/Media/Photos", &["Öffentlich/**"])];
        assert_eq!(
            decide(&unit("/Media/Photos/öffentlich/secret.jpg", None), &folders),
            Admission::Admitted
        );
        assert_eq!(
            decide(&unit("/Media/Photos/Öffentlich/secret.jpg", None), &folders),
            Admission::Excluded
        );
    }

    #[test]
    fn admitted_assets_preserves_discovery_order() {
        let paths = ["C:/Photos/z.jpg", "C:/Photos/a.jpg", "C:/Photos/m.jpg"];
        let admitted = admitted_assets(paths.iter().map(|path| asset(path)).collect(), &[]);

        assert_eq!(
            admitted
                .iter()
                .map(|asset| asset.path.as_str())
                .collect::<Vec<_>>(),
            paths
        );
    }
}
