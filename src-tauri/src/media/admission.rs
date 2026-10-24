use crate::db::WatchedFolder;
use crate::media::model::UploadUnit;

fn normalized_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn relative_path(folder: &str, path: &str) -> Option<String> {
    let normalized_folder = normalized_path(folder);
    let normalized_path = normalized_path(path);
    let relative_start = normalized_folder.len();
    (normalized_path == normalized_folder
        || normalized_path.starts_with(&(normalized_folder + "/")))
    .then(|| normalized_path[relative_start..].to_owned())
}

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

fn matches_pattern(pattern: &str, relative_path: &str) -> bool {
    let pattern = normalized_path(pattern);
    let path = normalized_path(relative_path);
    let pattern_segments = pattern
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let path_segments = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

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

    matches(&pattern_segments, &path_segments)
}

fn folder_for_path<'a>(
    path: &str,
    folders: &'a [WatchedFolder],
) -> Option<(&'a WatchedFolder, String)> {
    folders
        .iter()
        .filter_map(|folder| relative_path(&folder.path, path).map(|relative| (folder, relative)))
        .max_by_key(|(folder, _)| normalized_path(&folder.path).len())
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
    unit.assets()
        .any(|asset| path_is_excluded(&asset.path, folders))
        .then_some(Admission::Excluded)
        .unwrap_or(Admission::Admitted)
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
}
