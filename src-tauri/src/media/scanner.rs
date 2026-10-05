use super::extensions::is_media_file;
use super::model::{calculate_checksums, Asset, Checksums, FileFailure, ScanResult};
use super::paths::overlapping_folder_paths;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_STABLE_HASH_ATTEMPTS: usize = 3;
const _: () = assert!(MAX_STABLE_HASH_ATTEMPTS > 0);

pub(crate) fn metadata_mtime(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
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
    if expected_mtime == 0 || mtime == 0 || (size, mtime) != (expected_size, expected_mtime) {
        return Ok(None);
    }
    Ok(Some((
        Checksums {
            sha1_base64: cached_sha1_base64.to_string(),
        },
        size,
        mtime,
    )))
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
    let mut result = ScanResult::default();
    for entry in walkdir::WalkDir::new(path) {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_media_file(entry.path()) => {
                let file_path = entry.path().to_string_lossy().to_string();
                match entry.metadata() {
                    Ok(metadata) => result.files.push(Asset {
                        path: file_path,
                        size: metadata.len(),
                        mtime: metadata_mtime(&metadata),
                    }),
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

pub(crate) async fn scan_paths_for_media(paths: Vec<PathBuf>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            if !is_media_file(&path) {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => result.files.push(Asset {
                    path: path.to_string_lossy().to_string(),
                    size: metadata.len(),
                    mtime: metadata_mtime(&metadata),
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

pub(crate) async fn prepare_failed_sync_retry(paths: Vec<String>) -> Result<ScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut result = ScanResult::default();
        for path in paths {
            let path_buf = PathBuf::from(&path);
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
                    result.failures.push(FileFailure {
                        path,
                        error: "File no longer exists.".to_string(),
                    })
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
        let result = prepare_failed_sync_retry(vec![
            media_path.to_string_lossy().to_string(),
            unsupported_path.to_string_lossy().to_string(),
            directory_path.to_string_lossy().to_string(),
            missing_path.to_string_lossy().to_string(),
        ])
        .await
        .unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, media_path.to_string_lossy());
        assert_eq!(result.failures.len(), 3);
        assert!(result
            .failures
            .iter()
            .any(|failure| failure.error == "Path is no longer a supported media file."));
        assert!(result
            .failures
            .iter()
            .any(|failure| failure.error == "Path is no longer a regular file."));
        assert!(result
            .failures
            .iter()
            .any(|failure| failure.error == "File no longer exists."));
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
    fn cached_checksum_propagates_missing_file_errors() {
        let path = temporary_directory().join("missing.jpg");
        let error =
            reuse_cached_checksums(path.to_str().unwrap(), 0, 1, "cached-sha1").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
