use serde::Deserialize;
use std::io;
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
    let sidecar_path = path_buf.with_extension("xmp");
    let sidecar_file = match tokio::fs::File::open(&sidecar_path).await {
        Ok(file) => Some(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Cannot open sidecar {}: {}",
                sidecar_path.display(),
                error
            ))
        }
    };
    if let Some(sidecar_file) = sidecar_file {
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
    use super::{parse_upload_response, validate_sidecar_size, MAX_SIDECAR_BYTES};

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
}
