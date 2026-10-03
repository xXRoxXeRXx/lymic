use base64::{engine::general_purpose, Engine as _};
use reqwest::{Client, StatusCode};
use serde_json::json;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fmt;
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

#[derive(Debug)]
pub enum BulkCheckError {
    Retryable(String),
    NonRetryable(String),
}

impl BulkCheckError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

impl fmt::Display for BulkCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(message) | Self::NonRetryable(message) => message.fmt(formatter),
        }
    }
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


    pub async fn check_assets_exist(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<String>, BulkCheckError> {
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
            .map_err(|_| BulkCheckError::Retryable("Check request failed".to_string()))?;

        if !response.status().is_success() {
            return Err(bulk_check_status_error(response.status()));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", e)))?;

        parse_bulk_check_response(&data, &hashes).map_err(BulkCheckError::Retryable)
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

fn bulk_check_status_error(status: StatusCode) -> BulkCheckError {
    let message = format!("Server error during bulk check ({})", status);
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        BulkCheckError::NonRetryable(message)
    } else {
        BulkCheckError::Retryable(message)
    }
}

fn parse_bulk_check_response(
    data: &serde_json::Value,
    requested_hashes: &[String],
) -> Result<Vec<String>, String> {
    let results = data
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or("Invalid bulk-check response: missing results array")?;

    if results.len() != requested_hashes.len() {
        return Err(format!(
            "Invalid bulk-check response: expected {} results, received {}",
            requested_hashes.len(),
            results.len()
        ));
    }

    let mut expected: HashMap<&str, usize> = HashMap::new();
    for hash in requested_hashes {
        *expected.entry(hash).or_default() += 1;
    }

    let mut existing = Vec::new();
    for result in results {
        let id = result
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid bulk-check response: result missing string id")?;
        let action = result
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid bulk-check response: result missing string action")?;

        let remaining = expected
            .get_mut(id)
            .ok_or("Invalid bulk-check response: unexpected result id")?;
        if *remaining == 0 {
            return Err("Invalid bulk-check response: duplicate result id".to_string());
        }
        *remaining -= 1;

        match action {
            "accept" => {}
            "reject" => existing.push(id.to_string()),
            _ => return Err(format!("Invalid bulk-check response: unknown action '{}'", action)),
        }
    }

    if expected.values().any(|remaining| *remaining != 0) {
        return Err("Invalid bulk-check response: response omitted requested assets".to_string());
    }

    Ok(existing)
}

#[cfg(test)]
mod tests {
    use super::{bulk_check_status_error, parse_bulk_check_response, ImmichClient};
    use reqwest::StatusCode;
    use serde_json::json;

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

    #[test]
    fn bulk_check_requires_a_complete_valid_response() {
        let hashes = vec!["first".to_string(), "second".to_string()];
        let valid = json!({
            "results": [
                { "id": "first", "action": "reject" },
                { "id": "second", "action": "accept" }
            ]
        });
        assert_eq!(
            parse_bulk_check_response(&valid, &hashes).unwrap(),
            vec!["first".to_string()]
        );

        for invalid in [
            json!({}),
            json!({ "results": [] }),
            json!({ "results": [{ "id": "first", "action": "accept" }, { "id": "unknown", "action": "accept" }] }),
            json!({ "results": [{ "id": "first", "action": "accept" }, { "id": "first", "action": "reject" }] }),
            json!({ "results": [{ "id": "first", "action": "ignore" }, { "id": "second", "action": "accept" }] }),
        ] {
            assert!(parse_bulk_check_response(&invalid, &hashes).is_err());
        }
    }

    #[test]
    fn authentication_errors_are_not_retryable() {
        assert!(!bulk_check_status_error(StatusCode::UNAUTHORIZED).is_retryable());
        assert!(!bulk_check_status_error(StatusCode::FORBIDDEN).is_retryable());
        assert!(bulk_check_status_error(StatusCode::INTERNAL_SERVER_ERROR).is_retryable());
    }
}

