use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json::json;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{self, Read};
use tokio_util::io::ReaderStream;
use url::Url;

/// Calculate the SHA-1 hash of a file, returned as standard base64.
///
/// Immich's bulk-upload-check endpoint and x-immich-checksum header both
/// expect SHA-1 encoded as standard base64 (RFC 4648 §4), which is what
/// `general_purpose::STANDARD.encode` produces. (Documented)
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

#[derive(Debug, Clone)]
pub struct SyncAsset {
    pub path: String,
    pub size: u64,
    pub hash: Option<String>,
    pub mtime: i64,
}

pub struct ImmichClient {
    client: Client,
    server_url: Url,
    api_key: String,
}

impl ImmichClient {
    pub fn new(server_url: String, api_key: String) -> Result<Self, String> {
        let mut base_url = Url::parse(server_url.trim())
            .map_err(|_| "Invalid server URL".to_string())?;

        if base_url.scheme() != "https" {
            return Err("Only HTTPS server URLs are supported".to_string());
        }
        if base_url.host().is_none() {
            return Err("Server URL must include a host".to_string());
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err("Server URL must not contain credentials, query, or fragment".to_string());
        }

        let path = base_url.path().trim_end_matches('/');
        let api_path = if path.ends_with("/api") {
            format!("{}/", path)
        } else if path.is_empty() {
            "/api/".to_string()
        } else {
            format!("{}/api/", path)
        };
        base_url.set_path(&api_path);

        Ok(Self {
            // Reject redirects so an HTTPS endpoint cannot downgrade an API-key request to HTTP.
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "Failed to initialize HTTP client".to_string())?,
            server_url: base_url,
            api_key,
        })
    }

    fn endpoint_url(&self, path: &str) -> Url {
        self.server_url
            .join(path.trim_start_matches('/'))
            .expect("validated base URL must support relative endpoint paths")
    }

    pub async fn validate_connection(&self) -> Result<(), String> {
        let response = self
            .client
            .get(self.endpoint_url("server/config"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|_| "Connection failed".to_string())?;

        if !response.status().is_success() {
            return Err(format!("Server returned error: {}", response.status()));
        }

        Ok(())
    }


    pub async fn check_assets_exist(&self, hashes: Vec<String>) -> Result<Vec<String>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let url = self.endpoint_url("assets/bulk-upload-check");
        
        // Correct DTO: { "assets": [ { "id": "...", "checksum": "..." } ] }
        let assets_items: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| json!({ "id": h, "checksum": h }))
            .collect();

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .json(&json!({ "assets": assets_items }))
            .send()
            .await
            .map_err(|_| "Check request failed".to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(format!("Server error during bulk check ({})", status));
        }

        let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        let mut existing = Vec::new();
        if let Some(results) = data.get("results") {
            if let Some(arr) = results.as_array() {
                for item in arr {
                    // Result DTO uses "id" to match the "id" sent in the request, 
                    // and "action" to indicate if it should be accepted or rejected.
                    if let (Some(id), Some(action)) =
                        (item.get("id"), item.get("action"))
                    {
                        if action.as_str() == Some("reject") {
                            existing.push(id.as_str().unwrap_or_default().to_string());
                        }
                    }
                }
            }
        }
        Ok(existing)
    }

    /// Upload a single asset.
    pub async fn upload_asset(
        &self,
        path: &str,
        device_id: &str,
        precomputed_hash: &str,
    ) -> Result<String, String> {
        let url = self.endpoint_url("assets");
        let path_buf = std::path::PathBuf::from(path);
        let file_name = path_buf
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

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

        let sidecar_path = path_buf.with_extension("xmp");
        if sidecar_path.exists() {
            let sidecar_name = sidecar_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.xmp")
                .to_string();
            let sidecar_bytes =
                std::fs::read(&sidecar_path).map_err(|e| e.to_string())?;
            form = form.part(
                "sidecarData",
                reqwest::multipart::Part::bytes(sidecar_bytes).file_name(sidecar_name),
            );
        }

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", precomputed_hash)
            .timeout(std::time::Duration::from_secs(300))
            .multipart(form)
            .send()
            .await
            .map_err(|_| "Network error during upload".to_string())?;

        if response.status().is_success() {
            let data: serde_json::Value =
                response.json().await.map_err(|e| e.to_string())?;
            Ok(data["id"].as_str().unwrap_or("").to_string())
        } else {
            let status = response.status();
            Err(format!("Upload failed: {}", status))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ImmichClient;

    #[test]
    fn rejects_insecure_or_sensitive_server_urls() {
        for server_url in [
            "http://immich.example",
            "https://user:password@immich.example",
            "https://immich.example/?token=secret",
            "https://immich.example/#fragment",
        ] {
            assert!(ImmichClient::new(server_url.to_string(), "key".to_string()).is_err());
        }
    }

    #[test]
    fn normalizes_secure_server_urls() {
        let cases = [
            ("https://immich.example", "https://immich.example/api/server/config"),
            ("https://immich.example/", "https://immich.example/api/server/config"),
            ("https://immich.example/api", "https://immich.example/api/server/config"),
            ("https://immich.example/api/", "https://immich.example/api/server/config"),
            (
                "https://immich.example/custom/path",
                "https://immich.example/custom/path/api/server/config",
            ),
            (
                "https://immich.example:8443",
                "https://immich.example:8443/api/server/config",
            ),
        ];

        for (server_url, expected_endpoint) in cases {
            let client = ImmichClient::new(server_url.to_string(), "key".to_string())
                .expect("HTTPS URL should be accepted");
            assert_eq!(client.endpoint_url("/server/config").as_str(), expected_endpoint);
        }
    }
}

