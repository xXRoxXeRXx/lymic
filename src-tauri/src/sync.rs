use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json::json;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{self, Read};
use tokio_util::io::ReaderStream;

/// Calculate the SHA-1 hash of a file, returned as standard base64.
///
/// Immich's bulk-upload-check endpoint and x-immich-checksum header both
/// expect SHA-1 encoded as standard base64 (RFC 4648 §4), which is what
/// `general_purpose::STANDARD.encode` produces. (Fix 16 — documented)
pub fn calculate_hash(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let result = hasher.finalize();
    Ok(general_purpose::STANDARD.encode(result))
}

pub struct ImmichClient {
    client: Client,
    server_url: String,
    api_key: String,
}

impl ImmichClient {
    pub fn new(server_url: String, api_key: String) -> Self {
        let mut base_url = server_url.trim_end_matches('/').to_string();
        if !base_url.ends_with("/api") {
            base_url.push_str("/api");
        }
        Self {
            client: Client::new(),
            server_url: base_url,
            api_key,
        }
    }

    /// Expose the pooled reqwest Client so callers can reuse it (e.g. login
    /// connection test) without creating a new Client::new() each time. (Fix 15)
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Return the normalised server URL (with /api suffix).
    pub fn base_url(&self) -> &str {
        &self.server_url
    }

    pub async fn check_assets_exist(&self, hashes: Vec<String>) -> Result<Vec<String>, String> {
        let url = format!("{}/assets/bulk-upload-check", self.server_url);
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&json!({ "checksums": hashes }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        let mut existing = Vec::new();
        if let Some(results) = data.get("results") {
            if let Some(arr) = results.as_array() {
                for item in arr {
                    if let (Some(hash), Some(action)) =
                        (item.get("checksum"), item.get("action"))
                    {
                        if action.as_str() == Some("reject") {
                            existing.push(hash.as_str().unwrap_or_default().to_string());
                        }
                    }
                }
            }
        }
        Ok(existing)
    }

    /// Upload a single asset.
    ///
    /// `precomputed_hash` is the SHA-1 base64 already calculated by the caller,
    /// so we avoid hashing the file a second time. (Fix 4)
    ///
    /// The file is streamed directly from disk rather than buffered into RAM,
    /// so large videos (multi-GB) will not cause OOM. (Fix 3)
    ///
    /// # TOCTOU note
    /// File metadata is read before the file is opened for streaming. If the
    /// file is replaced between the stat and the open, the hash/size stored in
    /// the DB may not match the bytes actually sent. (Fix 12 — documented)
    pub async fn upload_asset(
        &self,
        path: &str,
        device_id: &str,
        precomputed_hash: &str,
    ) -> Result<String, String> {
        let url = format!("{}/assets", self.server_url);
        let path_buf = std::path::PathBuf::from(path);
        let file_name = path_buf
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        // Fix 10: propagate timestamp errors instead of unwrapping
        let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let created_at = metadata
            .created()
            .or_else(|_| metadata.modified())
            .map_err(|e| format!("Cannot read file timestamps for {}: {}", path, e))?;
        let modified_at = metadata.modified().unwrap_or(created_at);

        let created_at_iso = chrono::DateTime::<chrono::Utc>::from(created_at)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let modified_at_iso = chrono::DateTime::<chrono::Utc>::from(modified_at)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let file_size = metadata.len();

        // Fix 3: stream the file instead of loading it all into RAM
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| format!("Cannot open file {}: {}", path, e))?;
        let stream = ReaderStream::new(file);
        let asset_part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(stream),
            file_size,
        )
        .file_name(file_name);

        let mut form = reqwest::multipart::Form::new()
            .text("deviceAssetId", path.to_string())
            .text("deviceId", device_id.to_string())
            .text("fileCreatedAt", created_at_iso)
            .text("fileModifiedAt", modified_at_iso)
            .part("assetData", asset_part);

        // Check for sidecar file (.xmp)
        let sidecar_path = path_buf.with_extension("xmp");
        if sidecar_path.exists() {
            let sidecar_name = sidecar_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.xmp")
                .to_string();
            // Sidecar files are typically tiny, loading into memory is fine
            let sidecar_bytes =
                std::fs::read(&sidecar_path).map_err(|e| e.to_string())?;
            form = form.part(
                "sidecarData",
                reqwest::multipart::Part::bytes(sidecar_bytes).file_name(sidecar_name),
            );
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", precomputed_hash) // SHA-1 base64 (Fix 4, Fix 16)
            .timeout(std::time::Duration::from_secs(300)) // Increased for large files
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if response.status().is_success() {
            let data: serde_json::Value =
                response.json().await.map_err(|e| e.to_string())?;
            Ok(data["id"].as_str().unwrap_or("").to_string())
        } else {
            let status = response.status();
            // Fix 1: truncate server error bodies to avoid leaking sensitive header reflections
            let error_text = response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>();
            Err(format!("Upload failed: {} — {}", status, error_text))
        }
    }
}
