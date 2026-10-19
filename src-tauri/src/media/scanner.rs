use super::extensions::{is_media_file, live_photo_sibling_paths};
use super::model::{calculate_checksums, Asset, Checksums, FileFailure, ScanResult};
use super::paths::overlapping_folder_paths;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

const MAX_STABLE_HASH_ATTEMPTS: usize = 3;
const _: () = assert!(MAX_STABLE_HASH_ATTEMPTS > 0);

#[cfg(windows)]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0000_0002;

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| name.starts_with('.'))
}

#[cfg(windows)]
fn metadata_is_hidden(metadata: &std::fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

fn is_hidden_path(path: &Path) -> bool {
    if path
        .components()
        .any(|component| matches!(component, Component::Normal(name) if is_hidden_name(name)))
    {
        return true;
    }

    #[cfg(windows)]
    {
        std::fs::metadata(path)
            .map(|metadata| metadata_is_hidden(&metadata))
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn is_hidden_file_path(path: &Path) -> bool {
    is_hidden_path(path) || path.parent().is_some_and(is_hidden_path)
}

fn system_time_mtime(time: std::time::SystemTime) -> i64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_nanos()).unwrap_or(0),
        Err(error) => i64::try_from(error.duration().as_nanos())
            .ok()
            .and_then(|nanoseconds| nanoseconds.checked_neg())
            .unwrap_or(0),
    }
}

pub(crate) fn metadata_mtime(metadata: &std::fs::Metadata) -> i64 {
    // Zero is the cache sentinel for an unknown or unrepresentable file version.
    metadata.modified().map(system_time_mtime).unwrap_or(0)
}

pub(crate) fn file_version(path: &str) -> std::io::Result<(u64, i64)> {
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.len(), metadata_mtime(&metadata)))
}

pub(crate) fn reuse_cached_checksums(
    path: &str,
    expected_size: u64,
    expected_mtime: i64,
    cached_sha1_base64: &str,
) -> std::io::Result<Option<(Checksums, u64, i64)>> {
    let (size, mtime) = file_version(path)?;
    Ok(reuse_cached_checksums_for_version(
        expected_size,
        expected_mtime,
        size,
        mtime,
        cached_sha1_base64,
    ))
}

fn reuse_cached_checksums_for_version(
    expected_size: u64,
    expected_mtime: i64,
    size: u64,
    mtime: i64,
    cached_sha1_base64: &str,
) -> Option<(Checksums, u64, i64)> {
    if expected_mtime == 0 || mtime == 0 || (size, mtime) != (expected_size, expected_mtime) {
        return None;
    }
    Some((
        Checksums {
            sha1_base64: cached_sha1_base64.to_string(),
        },
        size,
        mtime,
    ))
}

pub(crate) fn calculate_stable_checksums(path: &str) -> std::io::Result<(Checksums, u64, i64)> {
    // Refresh the scan snapshot before hashing to avoid a redundant hash when it is stale.
    let mut version = file_version(path)?;
    for _ in 0..MAX_STABLE_HASH_ATTEMPTS {
        let checksums = calculate_checksums(path)?;
        let current_version = file_version(path)?;
        if current_version == version {
            return Ok((checksums, current_version.0, current_version.1));
        }
        version = current_version;
    }
    Err(std::io::Error::other("File changed while hashing"))
}

fn scan_folder_for_media(path: &Path) -> ScanResult {
    scan_folder_for_media_with_patterns(path, &[])
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

pub(crate) fn matches_exclusion_pattern(pattern: &str, relative_path: &Path) -> bool {
    let path = relative_path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
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
            Some((segment, rest)) => {
                path.first()
                    .is_some_and(|value| matches_segment(segment.as_bytes(), value.as_bytes()))
                    && matches(rest, &path[1..])
            }
        }
    }
    matches(&pattern_segments, &path_segments)
}

pub(crate) fn watched_folder_for_path<'a>(
    folders: &'a [crate::db::WatchedFolder],
    path: &Path,
) -> Option<&'a crate::db::WatchedFolder> {
    folders
        .iter()
        .filter(|folder| path.strip_prefix(&folder.path).is_ok())
        .max_by_key(|folder| Path::new(&folder.path).components().count())
}

pub(crate) fn path_matches_watched_folder_exclusion(
    path: &Path,
    folders: &[crate::db::WatchedFolder],
) -> bool {
    watched_folder_for_path(folders, path).is_some_and(|folder| {
        path.strip_prefix(&folder.path)
            .ok()
            .is_some_and(|relative| {
                folder
                    .exclusion_patterns
                    .iter()
                    .any(|pattern| matches_exclusion_pattern(pattern, relative))
            })
    })
}

/// An exclusion applies to the complete Upload Unit, never just one component.
pub(crate) fn upload_unit_matches_watched_folder_exclusion(
    path: &Path,
    folders: &[crate::db::WatchedFolder],
) -> bool {
    path_matches_watched_folder_exclusion(path, folders)
        || live_photo_sibling_paths(path)
            .into_iter()
            .any(|sibling| path_matches_watched_folder_exclusion(&sibling, folders))
}

fn scan_folder_for_media_with_patterns(path: &Path, exclusion_patterns: &[String]) -> ScanResult {
    let mut result = ScanResult::default();
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if is_hidden_name(entry.file_name())
                || matches!(name.as_ref(), "@eaDir" | "@SynoResource" | "#recycle")
            {
                return false;
            }

            #[cfg(windows)]
            if entry.file_type().is_dir() {
                return entry
                    .metadata()
                    .map(|metadata| !metadata_is_hidden(&metadata))
                    .unwrap_or(true);
            }

            true
        })
    {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_media_file(entry.path()) => {
                if entry
                    .path()
                    .strip_prefix(path)
                    .ok()
                    .is_some_and(|relative| {
                        exclusion_patterns
                            .iter()
                            .any(|pattern| matches_exclusion_pattern(pattern, relative))
                    })
                {
                    continue;
                }
                let file_path = entry.path().to_string_lossy().to_string();
                match entry.metadata() {
                    Ok(metadata) => {
                        #[cfg(windows)]
                        if metadata_is_hidden(&metadata) {
                            continue;
                        }
                        result.files.push(Asset {
                            path: file_path,
                            size: metadata.len(),
                            mtime: metadata_mtime(&metadata),
                            target_album_id: None,
                            remote_id: None,
                        });
                    }
                    Err(error) => result.failures.push(FileFailure {
                        path: file_path,
                        error: format!("Could not read file metadata: {}", error),
                    }),
                }
            }
            Ok(_) => {}
            Err(error) => result.failures.push(FileFailure {
                path: error.path().unwrap_or(path).to_string_lossy().to_string(),
                error: format!("Could not scan path: {}", error),
            }),
        }
    }
    result
}

pub(crate) async fn scan_folders_for_media(paths: Vec<PathBuf>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        let (paths, overlapping_folders) = overlapping_folder_paths(paths);
        let mut seen_files = HashSet::new();
        for path in paths {
            let scan_result = scan_folder_for_media(&path);
            for asset in scan_result.files {
                if seen_files.insert(PathBuf::from(&asset.path)) {
                    result.files.push(asset);
                }
            }
            result.failures.extend(scan_result.failures);
        }
        result.overlapping_folders = overlapping_folders;
        result
    })
    .await
    .map_err(|e| format!("Media scan task failed: {}", e))
}

pub(crate) async fn scan_watched_folders_for_media(
    folders: Vec<crate::db::WatchedFolder>,
) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let paths = folders
            .iter()
            .map(|folder| PathBuf::from(&folder.path))
            .collect();
        let (paths, overlapping_folders) = overlapping_folder_paths(paths);
        let mut result = ScanResult {
            overlapping_folders,
            ..Default::default()
        };
        let mut seen_files = HashSet::new();
        for path in paths {
            let patterns = folders
                .iter()
                .find(|folder| PathBuf::from(&folder.path) == path)
                .map(|folder| folder.exclusion_patterns.as_slice())
                .unwrap_or(&[]);
            let scan_result = scan_folder_for_media_with_patterns(&path, patterns);
            for asset in scan_result.files {
                if seen_files.insert(PathBuf::from(&asset.path)) {
                    result.files.push(asset);
                }
            }
            result.failures.extend(scan_result.failures);
        }
        result.files.retain(|asset| {
            !upload_unit_matches_watched_folder_exclusion(Path::new(&asset.path), &folders)
        });
        result
    })
    .await
    .map_err(|error| format!("Media scan task failed: {}", error))
}

pub(crate) async fn scan_paths_for_media(paths: Vec<PathBuf>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            if is_hidden_file_path(&path) || !is_media_file(&path) {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => result.files.push(Asset {
                    path: path.to_string_lossy().to_string(),
                    size: metadata.len(),
                    mtime: metadata_mtime(&metadata),
                    target_album_id: None,
                    remote_id: None,
                }),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => result.failures.push(FileFailure {
                    path: path.to_string_lossy().to_string(),
                    error: format!("Could not read file metadata: {}", error),
                }),
            }
        }
        result
    })
    .await
    .map_err(|e| format!("Media path scan task failed: {}", e))
}

pub(crate) async fn prepare_failed_sync_retry(
    paths: Vec<String>,
    folders: Vec<crate::db::WatchedFolder>,
) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            let path_buf = PathBuf::from(&path);
            if upload_unit_matches_watched_folder_exclusion(&path_buf, &folders) {
                result.discarded_paths.push(path);
                continue;
            }
            if !is_media_file(&path_buf) {
                result.failures.push(FileFailure {
                    path,
                    error: "Path is no longer a supported media file.".to_string(),
                });
                continue;
            }
            match std::fs::metadata(&path_buf) {
                Ok(metadata) if metadata.is_file() => match std::fs::File::open(&path_buf) {
                    Ok(_) => result.files.push(Asset {
                        path,
                        size: metadata.len(),
                        mtime: metadata_mtime(&metadata),
                        target_album_id: None,
                        remote_id: None,
                    }),
                    Err(error) => result.failures.push(FileFailure {
                        path,
                        error: format!("Could not open file for retry: {}", error),
                    }),
                },
                Ok(_) => result.failures.push(FileFailure {
                    path,
                    error: "Path is no longer a regular file.".to_string(),
                }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    result.discarded_paths.push(path)
                }
                Err(error) => result.failures.push(FileFailure {
                    path,
                    error: format!("Could not read file metadata for retry: {}", error),
                }),
            }
        }
        result
    })
    .await
    .map_err(|error| format!("Failed to prepare retry: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn temporary_directory() -> PathBuf {
        std::env::temp_dir().join(format!("lymic-scan-test-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn overlapping_folders_scan_each_media_file_once() {
        let root = temporary_directory();
        let child = root.join("Vacation");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("photo.jpg"), b"test image").unwrap();
        let result = scan_folders_for_media(vec![root.clone(), child])
            .await
            .unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.overlapping_folders.len(), 1);
        assert!(!result.files[0].path.starts_with(r"\\?\"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn folder_scan_skips_hidden_and_nas_metadata_entries() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let root = crate::media::paths::normalize_folder_path(&root);
        let media_path = root.join("Vacation").join("photo.jpg");
        let excluded_media_paths = [
            root.join(".cache").join("cached.jpg"),
            root.join("@eaDir").join("thumbnail.jpg"),
            root.join("Vacation")
                .join("@eaDir")
                .join("SYNOPHOTO_THUMB_SM.jpg"),
            root.join("@SynoResource").join("resource.jpg"),
            root.join("#recycle").join("deleted.jpg"),
            root.join(".hidden.jpg"),
        ];

        std::fs::create_dir_all(media_path.parent().unwrap()).unwrap();
        std::fs::write(&media_path, b"test image").unwrap();
        for path in &excluded_media_paths {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, b"excluded image").unwrap();
        }

        let result = scan_folders_for_media(vec![root.clone()]).await.unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, media_path.to_string_lossy());
        assert!(result.failures.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_retry_skips_media_matching_current_folder_exclusion_patterns() {
        let root = temporary_directory();
        let excluded = root.join("Screenshots").join("private.jpg");
        let included = root.join("Trips").join("keep.jpg");
        std::fs::create_dir_all(excluded.parent().unwrap()).unwrap();
        std::fs::create_dir_all(included.parent().unwrap()).unwrap();
        std::fs::write(&excluded, b"excluded image").unwrap();
        std::fs::write(&included, b"included image").unwrap();

        let result = prepare_failed_sync_retry(
            vec![
                excluded.to_string_lossy().to_string(),
                included.to_string_lossy().to_string(),
            ],
            vec![crate::db::WatchedFolder {
                id: 1,
                path: root.to_string_lossy().to_string(),
                recursive: true,
                target_album_id: None,
                exclusion_patterns: vec!["screenshots/**".to_string()],
            }],
        )
        .await
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, included.to_string_lossy());
        assert_eq!(result.discarded_paths, vec![excluded.to_string_lossy()]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn watched_folder_scan_excluding_one_live_photo_component_discards_the_upload_unit() {
        let root = temporary_directory();
        let image = root.join("IMG_1.HEIC");
        let video = root.join("IMG_1.MOV");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&video, b"video").unwrap();

        let result = scan_watched_folders_for_media(vec![crate::db::WatchedFolder {
            id: 1,
            path: root.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: vec!["*.mov".to_string()],
        }])
        .await
        .unwrap();

        assert!(result.files.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_retry_excluding_a_live_photo_video_discards_its_image_upload_unit() {
        let root = temporary_directory();
        let image = root.join("IMG_2.HEIC");
        let video = root.join("IMG_2.MOV");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&video, b"video").unwrap();

        let result = prepare_failed_sync_retry(
            vec![image.to_string_lossy().to_string()],
            vec![crate::db::WatchedFolder {
                id: 1,
                path: root.to_string_lossy().to_string(),
                recursive: true,
                target_album_id: None,
                exclusion_patterns: vec!["*.mov".to_string()],
            }],
        )
        .await
        .unwrap();

        assert!(result.files.is_empty());
        assert_eq!(
            result.discarded_paths,
            vec![image.to_string_lossy().to_string()]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusion_patterns_match_relative_paths_case_insensitively() {
        assert!(matches_exclusion_pattern(
            "screenshots/**",
            Path::new("Screenshots/ONE.JPG")
        ));
        assert!(matches_exclusion_pattern(
            "**/*.jpg",
            Path::new("Trips/photo.JPG")
        ));
        assert!(!matches_exclusion_pattern(
            "screenshots/**",
            Path::new("Trips/photo.jpg")
        ));
    }

    #[tokio::test]
    async fn path_scan_skips_hidden_files_and_hidden_parent_directories() {
        let root = temporary_directory();
        let visible_path = root.join("Vacation").join("photo.jpg");
        let hidden_file_path = root.join(".hidden.jpg");
        let hidden_directory_path = root.join(".cache").join("cached.jpg");

        std::fs::create_dir_all(visible_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(hidden_directory_path.parent().unwrap()).unwrap();
        std::fs::write(&visible_path, b"visible image").unwrap();
        std::fs::write(&hidden_file_path, b"hidden image").unwrap();
        std::fs::write(&hidden_directory_path, b"hidden image").unwrap();

        let result = scan_paths_for_media(vec![
            visible_path.clone(),
            hidden_file_path,
            hidden_directory_path,
        ])
        .await
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, visible_path.to_string_lossy());
        assert!(result.failures.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    fn set_hidden_attribute(path: &Path, hidden: bool) {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;

        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let current_attributes = std::fs::metadata(path).unwrap().file_attributes();
        let attributes = if hidden {
            current_attributes | FILE_ATTRIBUTE_HIDDEN
        } else {
            current_attributes & !FILE_ATTRIBUTE_HIDDEN
        };
        assert_ne!(
            unsafe { SetFileAttributesW(wide_path.as_ptr(), attributes) },
            0
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn scans_skip_windows_hidden_files_and_directories() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let root = crate::media::paths::normalize_folder_path(&root);
        let visible_path = root.join("Vacation").join("photo.jpg");
        let hidden_file_path = root.join("hidden.jpg");
        let hidden_directory = root.join("hidden-directory");
        let hidden_directory_path = hidden_directory.join("cached.jpg");

        std::fs::create_dir_all(visible_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&hidden_directory).unwrap();
        std::fs::write(&visible_path, b"visible image").unwrap();
        std::fs::write(&hidden_file_path, b"hidden image").unwrap();
        std::fs::write(&hidden_directory_path, b"hidden image").unwrap();
        set_hidden_attribute(&hidden_file_path, true);
        set_hidden_attribute(&hidden_directory, true);

        let folder_result = scan_folders_for_media(vec![root.clone()]).await.unwrap();
        let paths_result = scan_paths_for_media(vec![
            visible_path.clone(),
            hidden_file_path.clone(),
            hidden_directory_path.clone(),
        ])
        .await
        .unwrap();

        assert_eq!(folder_result.files.len(), 1);
        assert_eq!(folder_result.files[0].path, visible_path.to_string_lossy());
        assert_eq!(paths_result.files.len(), 1);
        assert_eq!(paths_result.files[0].path, visible_path.to_string_lossy());

        set_hidden_attribute(&hidden_file_path, false);
        set_hidden_attribute(&hidden_directory, false);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn retry_preparation_keeps_invalid_paths_as_failures() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let media_path = root.join("photo.jpg");
        let unsupported_path = root.join("notes.txt");
        let directory_path = root.join("directory.jpg");
        let missing_path = root.join("missing.jpg");
        std::fs::write(&media_path, b"test image").unwrap();
        std::fs::write(&unsupported_path, b"not media").unwrap();
        std::fs::create_dir_all(&directory_path).unwrap();
        let result = prepare_failed_sync_retry(
            vec![
                media_path.to_string_lossy().to_string(),
                unsupported_path.to_string_lossy().to_string(),
                directory_path.to_string_lossy().to_string(),
                missing_path.to_string_lossy().to_string(),
            ],
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, media_path.to_string_lossy());
        assert_eq!(result.failures.len(), 2);
        assert_eq!(
            result.discarded_paths,
            vec![missing_path.to_string_lossy().to_string()]
        );
        assert!(result
            .failures
            .iter()
            .any(|failure| failure.error == "Path is no longer a supported media file."));
        assert!(result
            .failures
            .iter()
            .any(|failure| failure.error == "Path is no longer a regular file."));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stable_checksums_use_the_hashed_file_version() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"test image").unwrap();
        let path_string = path.to_string_lossy().to_string();
        let (checksums, size, mtime) = calculate_stable_checksums(&path_string).unwrap();
        assert_eq!(checksums, calculate_checksums(&path_string).unwrap());
        assert_eq!((size, mtime), file_version(&path_string).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn system_time_mtime_preserves_pre_epoch_nanoseconds() {
        assert_eq!(
            system_time_mtime(UNIX_EPOCH - Duration::from_nanos(100)),
            -100
        );
    }

    #[test]
    fn system_time_mtime_handles_boundaries_and_sentinel() {
        assert_eq!(system_time_mtime(UNIX_EPOCH), 0);
        assert_eq!(
            system_time_mtime(UNIX_EPOCH + Duration::from_nanos(100)),
            100
        );
        assert_eq!(
            system_time_mtime(
                UNIX_EPOCH + Duration::from_secs(i64::MAX as u64 / 1_000_000_000 + 1)
            ),
            0
        );
    }

    #[test]
    fn stable_checksums_use_current_version_when_file_changes_after_scan() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"old").unwrap();
        let path_string = path.to_string_lossy().to_string();
        let (old_size, _) = file_version(&path_string).unwrap();
        std::fs::write(&path, b"updated image content").unwrap();
        let (checksums, size, mtime) = calculate_stable_checksums(&path_string).unwrap();
        assert_eq!(checksums, calculate_checksums(&path_string).unwrap());
        assert_eq!((size, mtime), file_version(&path_string).unwrap());
        assert_ne!(size, old_size);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cached_checksum_is_reused_when_file_metadata_matches() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"test image").unwrap();
        let path_string = path.to_string_lossy().to_string();
        let (size, mtime) = file_version(&path_string).unwrap();

        let reused = reuse_cached_checksums(&path_string, size, mtime, "cached-sha1")
            .unwrap()
            .unwrap();

        assert_eq!(reused.0.sha1_base64, "cached-sha1");
        assert_eq!((reused.1, reused.2), (size, mtime));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cached_checksum_is_not_reused_after_metadata_changes() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"old").unwrap();
        let path_string = path.to_string_lossy().to_string();
        let (size, mtime) = file_version(&path_string).unwrap();
        std::fs::write(&path, b"updated image content").unwrap();

        assert!(
            reuse_cached_checksums(&path_string, size, mtime, "cached-sha1")
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cached_checksum_is_not_reused_with_an_unknown_expected_mtime() {
        let root = temporary_directory();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("photo.jpg");
        std::fs::write(&path, b"test image").unwrap();
        let path_string = path.to_string_lossy().to_string();
        let (size, _) = file_version(&path_string).unwrap();

        assert!(reuse_cached_checksums(&path_string, size, 0, "cached-sha1")
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cached_checksum_is_reused_with_matching_negative_mtime() {
        let reused = reuse_cached_checksums_for_version(123, -42, 123, -42, "cached-sha1").unwrap();

        assert_eq!(reused.0.sha1_base64, "cached-sha1");
        assert_eq!((reused.1, reused.2), (123, -42));
    }

    #[test]
    fn cached_checksum_propagates_missing_file_errors() {
        let path = temporary_directory().join("missing.jpg");
        let error =
            reuse_cached_checksums(path.to_str().unwrap(), 0, 1, "cached-sha1").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
