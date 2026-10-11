use futures::TryStreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::AsyncReadExt as _;
use tokio_util::io::ReaderStream;

use crate::sync::progress::TransferStatsTracker;

const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;

pub(super) struct UploadPayload {
    pub(super) form: reqwest::multipart::Form,
    pub(super) total_file_bytes: u64,
}

#[derive(Clone, Default)]
pub(crate) struct SidecarDirectoryCache {
    directories: Arc<Mutex<HashMap<PathBuf, Arc<DirectoryIndexSlot>>>>,
}

type DirectoryIndexResult = Result<Arc<DirectorySidecarIndex>, String>;
type DirectoryIndexSlot = OnceLock<DirectoryIndexResult>;

#[derive(Clone, Default)]
struct DirectorySidecarIndex {
    by_lowercase_name: HashMap<String, Vec<PathBuf>>,
}

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
}

pub(super) async fn upload_form(
    path: &str,
    live_photo_path: Option<&str>,
    transfer_tracker: Option<TransferStatsTracker>,
    sidecar_cache: SidecarDirectoryCache,
) -> Result<UploadPayload, String> {
    let path_buf = std::path::PathBuf::from(path);
    let file_name = path_buf
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    let metadata_path = path_buf.clone();
    let (metadata, sidecar_path) = tokio::task::spawn_blocking(move || {
        let metadata = std::fs::metadata(&metadata_path).map_err(|error| error.to_string())?;
        let sidecar_path = sidecar_cache.find_sidecar_path(&metadata_path)?;
        Ok::<_, String>((metadata, sidecar_path))
    })
    .await
    .map_err(|error| format!("Filesystem task failed while preparing {}: {}", path, error))??;
    let created_at = metadata
        .created()
        .or_else(|_| metadata.modified())
        .map_err(|error| format!("Cannot read file timestamps for {}: {}", path, error))?;
    let modified_at = metadata.modified().unwrap_or(created_at);
    let created_at_iso = chrono::DateTime::<chrono::Utc>::from(created_at)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let modified_at_iso = chrono::DateTime::<chrono::Utc>::from(modified_at)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let asset_file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("Cannot open file {}: {}", path, error))?;
    let asset_part = reqwest::multipart::Part::stream_with_length(
        reqwest::Body::wrap_stream(observed_stream(asset_file, transfer_tracker.clone())),
        metadata.len(),
    )
    .file_name(file_name);
    let live_photo_part = if let Some(live_photo_path) = live_photo_path {
        let live_metadata = tokio::fs::metadata(live_photo_path)
            .await
            .map_err(|error| {
                format!(
                    "Cannot read live photo video metadata for {}: {}",
                    live_photo_path, error
                )
            })?;
        if !live_metadata.is_file() {
            return Err(format!(
                "Live photo video is not a regular file: {}",
                live_photo_path
            ));
        }
        let live_file = tokio::fs::File::open(live_photo_path)
            .await
            .map_err(|error| {
                format!(
                    "Cannot open live photo video {}: {}",
                    live_photo_path, error
                )
            })?;
        let live_name = std::path::Path::new(live_photo_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("live-photo.mov")
            .to_string();
        Some((
            reqwest::multipart::Part::stream_with_length(
                reqwest::Body::wrap_stream(observed_stream(live_file, transfer_tracker.clone())),
                live_metadata.len(),
            )
            .file_name(live_name),
            live_metadata.len(),
        ))
    } else {
        None
    };
    let mut form = reqwest::multipart::Form::new()
        .text("fileCreatedAt", created_at_iso)
        .text("fileModifiedAt", modified_at_iso)
        .part("assetData", asset_part);
    let mut total_file_bytes = metadata.len();
    if let Some((part, size)) = live_photo_part {
        form = form.part("livePhotoData", part);
        total_file_bytes = total_file_bytes.saturating_add(size);
    }
    let sidecar_file = match sidecar_path.as_ref() {
        Some(sidecar_path) => match tokio::fs::File::open(sidecar_path).await {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "Cannot open sidecar {}: {}",
                    sidecar_path.display(),
                    error
                ))
            }
        },
        None => None,
    };
    if let (Some(sidecar_path), Some(sidecar_file)) = (sidecar_path, sidecar_file) {
        let sidecar_name = sidecar_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file.xmp")
            .to_string();
        let sidecar_metadata = sidecar_file.metadata().await.map_err(|error| {
            format!(
                "Cannot read sidecar metadata for {}: {}",
                sidecar_name, error
            )
        })?;
        if sidecar_metadata.is_file() {
            let size = sidecar_metadata.len();
            validate_sidecar_size(size)?;
            form = form.part(
                "sidecarData",
                reqwest::multipart::Part::stream_with_length(
                    reqwest::Body::wrap_stream(ReaderStream::new(sidecar_file.take(size))),
                    size,
                )
                .file_name(sidecar_name),
            );
            total_file_bytes = total_file_bytes.saturating_add(size);
        }
    }
    Ok(UploadPayload {
        form,
        total_file_bytes,
    })
}

fn observed_stream(
    file: tokio::fs::File,
    transfer_tracker: Option<TransferStatsTracker>,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    ReaderStream::new(file).inspect_ok(move |chunk| {
        if let Some(transfer_stats) = &transfer_tracker {
            transfer_stats.record_chunk(chunk.len() as u64);
        }
    })
}

impl SidecarDirectoryCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn find_sidecar_path(&self, asset_path: &Path) -> Result<Option<PathBuf>, String> {
        let directory = asset_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let index_slot = {
            let mut directories = self
                .directories
                .lock()
                .map_err(|_| "XMP sidecar directory cache lock was poisoned".to_string())?;
            directories
                .entry(directory.to_path_buf())
                .or_insert_with(|| Arc::new(DirectoryIndexSlot::new()))
                .clone()
        };
        let index = index_slot
            .get_or_init(|| DirectorySidecarIndex::build(directory).map(Arc::new))
            .clone()?;
        index.find(asset_path)
    }
}

impl DirectorySidecarIndex {
    fn build(directory: &Path) -> Result<Self, String> {
        let entries = std::fs::read_dir(directory).map_err(|error| {
            format!(
                "Cannot scan directory {} for XMP sidecars: {}",
                directory.display(),
                error
            )
        })?;
        let mut by_lowercase_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "Cannot read directory entry in {} while indexing XMP sidecars: {}",
                    directory.display(),
                    error
                )
            })?;
            let entry_name = entry.file_name();
            let Some(entry_name) = entry_name.to_str() else {
                continue;
            };
            if !entry_name.to_ascii_lowercase().ends_with(".xmp") {
                continue;
            }
            let path = entry.path();
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                // A sidecar can disappear between read_dir and metadata. It was not part of
                // this job's snapshot, so ignore it rather than failing every upload here.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!(
                        "Cannot inspect XMP sidecar candidate {}: {}",
                        path.display(),
                        error
                    ));
                }
            };
            if metadata.is_file() {
                by_lowercase_name
                    .entry(entry_name.to_ascii_lowercase())
                    .or_default()
                    .push(path);
            }
        }
        for paths in by_lowercase_name.values_mut() {
            paths.sort();
        }
        Ok(Self { by_lowercase_name })
    }

    fn find(&self, asset_path: &Path) -> Result<Option<PathBuf>, String> {
        let candidates = sidecar_candidates(asset_path);
        for candidate in candidates {
            let Some(name) = candidate.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(matches) = self.by_lowercase_name.get(&name.to_ascii_lowercase()) else {
                continue;
            };
            if let Some(exact) = matches.iter().find(|path| {
                path.file_name().and_then(|entry_name| entry_name.to_str()) == Some(name)
            }) {
                return Ok(Some(exact.clone()));
            }
            if let Some(sidecar_path) = matches.first() {
                return Ok(Some(sidecar_path.clone()));
            }
        }
        Ok(None)
    }
}

fn sidecar_candidates(asset_path: &Path) -> Vec<PathBuf> {
    let stem_candidate = asset_path.with_extension("xmp");
    let mut full_name_candidate = asset_path.as_os_str().to_os_string();
    full_name_candidate.push(".xmp");
    let full_name_candidate = PathBuf::from(full_name_candidate);
    if stem_candidate == full_name_candidate {
        vec![stem_candidate]
    } else {
        vec![stem_candidate, full_name_candidate]
    }
}

fn validate_sidecar_size(size: u64) -> Result<(), String> {
    if size > MAX_SIDECAR_BYTES {
        return Err(format!(
            "XMP sidecar exceeds {} MiB limit (size: {} bytes)",
            MAX_SIDECAR_BYTES / (1024 * 1024),
            size
        ));
    }
    Ok(())
}

pub(super) fn parse_upload_response(body: &[u8]) -> Result<String, String> {
    let payload: UploadResponse = serde_json::from_slice(body)
        .map_err(|error| format!("Invalid upload response: {}", error))?;
    if payload.id.trim().is_empty() {
        return Err("Server response contains no asset id".to_string());
    }
    Ok(payload.id)
}

#[cfg(test)]
mod tests {
    use super::{
        observed_stream, parse_upload_response, sidecar_candidates, upload_form,
        validate_sidecar_size, SidecarDirectoryCache, MAX_SIDECAR_BYTES,
    };
    use crate::sync::progress::TransferStatsTracker;
    use futures::StreamExt;
    use std::path::{Path, PathBuf};

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-xmp-sidecar-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn create_file(directory: &Path, name: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, b"test").unwrap();
        path
    }

    fn find_sidecar_path(asset_path: &Path) -> Result<Option<PathBuf>, String> {
        SidecarDirectoryCache::new().find_sidecar_path(asset_path)
    }

    #[tokio::test]
    async fn observed_stream_counts_only_read_payload_bytes() {
        let directory = temporary_directory();
        let path = create_file(&directory, "photo.jpg");
        std::fs::write(&path, b"payload bytes").unwrap();
        let tracker = TransferStatsTracker::new(100, 0);
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut stream = Box::pin(observed_stream(file, Some(tracker.clone())));
        while let Some(chunk) = stream.next().await {
            chunk.unwrap();
        }
        assert_eq!(
            tracker.snapshot().transferred_bytes,
            b"payload bytes".len() as u64
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn upload_form_counts_only_attached_file_parts() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        std::fs::write(&asset, b"image data").unwrap();
        let asset_size = std::fs::metadata(&asset).unwrap().len();

        let payload = upload_form(
            asset.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        assert_eq!(payload.total_file_bytes, asset_size);
        drop(payload);

        let live_photo = create_file(&directory, "photo.mov");
        std::fs::write(&live_photo, b"live photo data").unwrap();
        let live_photo_size = std::fs::metadata(&live_photo).unwrap().len();
        let payload = upload_form(
            asset.to_str().unwrap(),
            Some(live_photo.to_str().unwrap()),
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        assert_eq!(payload.total_file_bytes, asset_size + live_photo_size);
        drop(payload);

        let sidecar = asset.with_extension("xmp");
        std::fs::write(&sidecar, b"sidecar data").unwrap();
        let sidecar_size = std::fs::metadata(&sidecar).unwrap().len();
        let payload = upload_form(
            asset.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        assert_eq!(payload.total_file_bytes, asset_size + sidecar_size);
        drop(payload);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn upload_response_requires_non_empty_id() {
        assert_eq!(
            parse_upload_response(br#"{"id":"test-uuid-123"}"#).unwrap(),
            "test-uuid-123"
        );
        for invalid in [
            br#"{"id":""}"#.as_slice(),
            br#"{"id":"   "}"#.as_slice(),
            br#"{"status":"ok"}"#.as_slice(),
            br#"{"id":123}"#.as_slice(),
            br#"invalid json"#.as_slice(),
        ] {
            assert!(parse_upload_response(invalid).is_err());
        }
    }

    #[test]
    fn sidecar_size_limit_allows_boundary_and_rejects_larger_files() {
        assert!(validate_sidecar_size(MAX_SIDECAR_BYTES).is_ok());
        let error = validate_sidecar_size(MAX_SIDECAR_BYTES + 1).unwrap_err();
        assert!(error.contains("exceeds"));
        assert!(error.contains(&(MAX_SIDECAR_BYTES + 1).to_string()));
    }

    #[test]
    fn sidecar_candidates_support_both_naming_schemes_without_duplicates() {
        assert_eq!(
            sidecar_candidates(Path::new("photo.jpg")),
            vec![PathBuf::from("photo.xmp"), PathBuf::from("photo.jpg.xmp")]
        );
        assert_eq!(
            sidecar_candidates(Path::new("photo")),
            vec![PathBuf::from("photo.xmp")]
        );
    }

    #[test]
    fn finds_no_sidecar_or_either_exact_naming_scheme() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        assert_eq!(find_sidecar_path(&asset).unwrap(), None);

        let stem_sidecar = create_file(&directory, "photo.xmp");
        assert_eq!(
            find_sidecar_path(&asset).unwrap(),
            Some(stem_sidecar.clone())
        );
        std::fs::remove_file(stem_sidecar).unwrap();

        let full_name_sidecar = create_file(&directory, "photo.jpg.xmp");
        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stem_sidecar_wins_over_full_name_sidecar() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        let stem_sidecar = create_file(&directory, "photo.xmp");
        create_file(&directory, "photo.jpg.xmp");

        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(stem_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cache_keeps_a_directory_snapshot_until_the_next_sync_job() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        let cache = SidecarDirectoryCache::new();

        assert_eq!(cache.find_sidecar_path(&asset).unwrap(), None);
        let sidecar = create_file(&directory, "photo.xmp");
        assert_eq!(cache.find_sidecar_path(&asset).unwrap(), None);
        assert_eq!(
            SidecarDirectoryCache::new()
                .find_sidecar_path(&asset)
                .unwrap(),
            Some(sidecar)
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ignores_non_regular_sidecar_candidates() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        std::fs::create_dir(directory.join("photo.xmp")).unwrap();
        let full_name_sidecar = create_file(&directory, "photo.jpg.xmp");

        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_ascii_case_insensitive_sidecars_using_their_actual_names() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        let stem_sidecar = create_file(&directory, "photo.XMP");
        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(stem_sidecar));
        std::fs::remove_file(directory.join("photo.XMP")).unwrap();

        let full_name_sidecar = create_file(&directory, "PHOTO.JPG.XmP");
        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ignores_non_regular_case_insensitive_matches() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        std::fs::create_dir(directory.join("PHOTO.XMP")).unwrap();
        let full_name_sidecar = create_file(&directory, "PHOTO.JPG.XMP");

        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exact_case_wins_and_case_insensitive_matches_are_sorted() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        let exact_sidecar = create_file(&directory, "photo.xmp");
        create_file(&directory, "PHOTO.XMP");
        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(exact_sidecar));
        std::fs::remove_file(directory.join("photo.xmp")).unwrap();

        let first_sorted_sidecar = directory.join("PHOTO.XMP");
        create_file(&directory, "Photo.Xmp");
        assert_eq!(
            find_sidecar_path(&asset).unwrap(),
            Some(first_sorted_sidecar)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
