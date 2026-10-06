use serde::Deserialize;
use std::io;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt as _;
use tokio_util::io::ReaderStream;

const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
}

pub(super) async fn upload_form(
    path: &str,
    live_photo_path: Option<&str>,
) -> Result<reqwest::multipart::Form, String> {
    let path_buf = std::path::PathBuf::from(path);
    let file_name = path_buf
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
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
        reqwest::Body::wrap_stream(ReaderStream::new(asset_file)),
        metadata.len(),
    )
    .file_name(file_name);
    let live_photo_part = if let Some(live_photo_path) = live_photo_path {
        let live_metadata = std::fs::metadata(live_photo_path).map_err(|error| {
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
        Some(
            reqwest::multipart::Part::stream_with_length(
                reqwest::Body::wrap_stream(ReaderStream::new(live_file)),
                live_metadata.len(),
            )
            .file_name(live_name),
        )
    } else {
        None
    };
    let mut form = reqwest::multipart::Form::new()
        .text("fileCreatedAt", created_at_iso)
        .text("fileModifiedAt", modified_at_iso)
        .part("assetData", asset_part);
    if let Some(part) = live_photo_part {
        form = form.part("livePhotoData", part);
    }
    let sidecar_path = find_sidecar_path(&path_buf)?;
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
        }
    }
    Ok(form)
}

fn find_sidecar_path(asset_path: &Path) -> Result<Option<PathBuf>, String> {
    let candidates = sidecar_candidates(asset_path);
    for candidate in &candidates {
        match std::fs::metadata(candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(candidate.clone())),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Cannot inspect XMP sidecar for {} at {}: {}",
                    asset_path.display(),
                    candidate.display(),
                    error
                ));
            }
        }
    }

    let parent = asset_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    let directory = parent.unwrap_or_else(|| Path::new("."));
    let entries = std::fs::read_dir(directory).map_err(|error| {
        format!(
            "Cannot scan directory {} for XMP sidecar of {}: {}",
            directory.display(),
            asset_path.display(),
            error
        )
    })?;

    let expected_names: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate.file_name().and_then(|name| name.to_str()))
        .collect();
    let mut matches: Vec<Vec<PathBuf>> = vec![Vec::new(); candidates.len()];
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Cannot read directory entry in {} while finding XMP sidecar for {}: {}",
                directory.display(),
                asset_path.display(),
                error
            )
        })?;
        let entry_name = entry.file_name();
        let Some(entry_name) = entry_name.to_str() else {
            continue;
        };
        for (index, expected_name) in expected_names.iter().enumerate() {
            if expected_name
                .is_some_and(|expected_name| entry_name.eq_ignore_ascii_case(expected_name))
            {
                let path = entry.path();
                let metadata = std::fs::metadata(&path).map_err(|error| {
                    format!(
                        "Cannot inspect XMP sidecar candidate {} for {}: {}",
                        path.display(),
                        asset_path.display(),
                        error
                    )
                })?;
                if metadata.is_file() {
                    matches[index].push(path);
                }
            }
        }
    }

    for mut schema_matches in matches {
        schema_matches.sort();
        if let Some(sidecar_path) = schema_matches.into_iter().next() {
            return Ok(Some(sidecar_path));
        }
    }
    Ok(None)
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
        find_sidecar_path, parse_upload_response, sidecar_candidates, validate_sidecar_size,
        MAX_SIDECAR_BYTES,
    };
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
    fn ignores_non_regular_sidecar_candidates() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        std::fs::create_dir(directory.join("photo.xmp")).unwrap();
        let full_name_sidecar = create_file(&directory, "photo.jpg.xmp");

        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(not(windows))]
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

    #[cfg(not(windows))]
    #[test]
    fn ignores_non_regular_case_insensitive_matches() {
        let directory = temporary_directory();
        let asset = create_file(&directory, "photo.jpg");
        std::fs::create_dir(directory.join("PHOTO.XMP")).unwrap();
        let full_name_sidecar = create_file(&directory, "PHOTO.JPG.XMP");

        assert_eq!(find_sidecar_path(&asset).unwrap(), Some(full_name_sidecar));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(not(windows))]
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
