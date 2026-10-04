use base64::{engine::general_purpose, Engine as _};
use md5::Md5;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use tokio::io::AsyncReadExt as _;
use tokio_util::io::ReaderStream;
use url::Url;

const MAX_ERROR_RESPONSE_BYTES: usize = 8 * 1024;
const MAX_SUCCESS_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksums {
    pub sha1_base64: String,
    pub md5_hex: String,
    pub sha256_hex: String,
}

/// Calculate every checksum required by the upload protocol and audit record
/// in one streaming pass. Immich continues to receive SHA-1/base64.
pub fn calculate_checksums(path: &str) -> io::Result<Checksums> {
    let mut file = File::open(path)?;
    let mut sha1 = Sha1::new();
    let mut md5 = Md5::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha1.update(&buffer[..n]);
        md5.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
    }
    Ok(Checksums {
        sha1_base64: general_purpose::STANDARD.encode(sha1.finalize()),
        md5_hex: format!("{:x}", md5.finalize()),
        sha256_hex: format!("{:x}", sha256.finalize()),
    })
}

#[derive(Debug, Clone)]
pub struct SyncAsset {
    pub path: String,
    pub size: u64,
    pub checksums: Option<Checksums>,
    pub mtime: i64,
}

pub struct ImmichClient {
    client: Client,
    server_url: Url,
    api_key: String,
}

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
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
        let mut base_url =
            Url::parse(server_url.trim()).map_err(|_| "Invalid server URL".to_string())?;

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
            // This is Immich's authenticated "current user" endpoint.  Do not use
            // bulk-upload-check for login validation: that endpoint can fail because
            // of upload-service/database issues even while the API key is valid.
            .get(self.endpoint_url("users/me"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|_| "Connection failed".to_string())?;

        if !response.status().is_success() {
            return Err(format!(
                "Server returned error: {}",
                response_status_error(response).await
            ));
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
            return Err(bulk_check_response_error(response).await);
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES)
            .await
            .map_err(|e| {
                BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", e))
            })?;
        let data: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
            BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", e))
        })?;

        parse_bulk_check_response(&data, &hashes).map_err(BulkCheckError::Retryable)
    }

    /// Upload a single asset.
    pub async fn upload_asset(&self, path: &str, precomputed_hash: &str) -> Result<String, String> {
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
            .text("fileCreatedAt", created_at_iso)
            .text("fileModifiedAt", modified_at_iso)
            .part("assetData", asset_part);

        let sidecar_path = path_buf.with_extension("xmp");
        let sidecar_file = match tokio::fs::File::open(&sidecar_path).await {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "Cannot open sidecar {}: {}",
                    sidecar_path.display(),
                    error
                ));
            }
        };
        if let Some(sidecar_file) = sidecar_file {
            let sidecar_name = sidecar_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.xmp")
                .to_string();
            let sidecar_metadata = sidecar_file
                .metadata()
                .await
                .map_err(|e| format!("Cannot read sidecar metadata for {}: {}", sidecar_name, e))?;
            if sidecar_metadata.is_file() {
                let sidecar_size = sidecar_metadata.len();
                validate_sidecar_size(sidecar_size)?;
                let sidecar_stream = ReaderStream::new(sidecar_file.take(sidecar_size));
                form = form.part(
                    "sidecarData",
                    reqwest::multipart::Part::stream_with_length(
                        reqwest::Body::wrap_stream(sidecar_stream),
                        sidecar_size,
                    )
                    .file_name(sidecar_name),
                );
            }
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
            let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
            parse_upload_response(&body)
        } else {
            Err(format!(
                "Upload failed: {}",
                response_status_error(response).await
            ))
        }
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

fn bulk_check_status_error(status: StatusCode) -> BulkCheckError {
    let message = format!("Server error during bulk check ({})", status);
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        BulkCheckError::NonRetryable(message)
    } else {
        BulkCheckError::Retryable(message)
    }
}

async fn bulk_check_response_error(response: Response) -> BulkCheckError {
    let status = response.status();
    let detail = response_error_detail(response).await;
    match bulk_check_status_error(status) {
        BulkCheckError::Retryable(message) => {
            BulkCheckError::Retryable(format!("{}{}", message, detail))
        }
        BulkCheckError::NonRetryable(message) => {
            BulkCheckError::NonRetryable(format!("{}{}", message, detail))
        }
    }
}

async fn response_status_error(response: Response) -> String {
    let status = response.status();
    format!("{}{}", status, response_error_detail(response).await)
}

async fn response_error_detail(response: Response) -> String {
    read_error_body_capped(response, MAX_ERROR_RESPONSE_BYTES).await
}

async fn read_error_body_capped(mut response: Response, max_bytes: usize) -> String {
    let mut body = Vec::with_capacity(max_bytes);
    let mut truncated = false;

    while let Ok(Some(chunk)) = response.chunk().await {
        let remaining = max_bytes.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }

    let text = String::from_utf8_lossy(&body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match (text.is_empty(), truncated) {
        (true, false) => String::new(),
        (true, true) => ": [truncated]".to_string(),
        (false, false) => format!(": {}", text),
        (false, true) => format!(": {}... [truncated]", text),
    }
}

async fn read_response_body_limited(
    mut response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!(
            "response body exceeds {} KiB limit",
            max_bytes / 1024
        ));
    }

    let capacity = response
        .content_length()
        .unwrap_or_default()
        .min(max_bytes as u64) as usize;
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(format!(
                "response body exceeds {} KiB limit",
                max_bytes / 1024
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_upload_response(body: &[u8]) -> Result<String, String> {
    let payload: UploadResponse =
        serde_json::from_slice(body).map_err(|e| format!("Invalid upload response: {}", e))?;
    if payload.id.trim().is_empty() {
        return Err("Server response contains no asset id".to_string());
    }
    Ok(payload.id)
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

        match (
            action,
            result.get("reason").and_then(serde_json::Value::as_str),
        ) {
            ("accept", None) | ("accept", Some(_)) => {}
            // Older Immich versions did not include a reason for rejected assets.
            ("reject", None) | ("reject", Some("duplicate")) => existing.push(id.to_string()),
            ("reject", Some("unsupported-format")) => {
                return Err(format!("Asset rejected as unsupported format: {}", id));
            }
            ("reject", Some(reason)) => {
                return Err(format!(
                    "Asset rejected with unknown reason '{}': {}",
                    reason, id
                ));
            }
            _ => {
                return Err(format!(
                    "Invalid bulk-check response: unknown action '{}'",
                    action
                ))
            }
        }
    }

    if expected.values().any(|remaining| *remaining != 0) {
        return Err("Invalid bulk-check response: response omitted requested assets".to_string());
    }

    Ok(existing)
}

#[cfg(test)]
mod tests {
    use super::{
        bulk_check_status_error, calculate_checksums, parse_bulk_check_response,
        parse_upload_response, read_error_body_capped, read_response_body_limited,
        validate_sidecar_size, ImmichClient, MAX_ERROR_RESPONSE_BYTES, MAX_SIDECAR_BYTES,
    };
    use crate::audit::{report_audit, AuditError};
    use reqwest::{Client, Response, StatusCode};
    use serde_json::json;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request");
            request.extend_from_slice(&chunk[..read]);
            if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length: ")
                    .or_else(|| line.strip_prefix("Content-Length: "))
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request body");
            request.extend_from_slice(&chunk[..read]);
        }
        request
    }

    fn temporary_file(contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-sync-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    async fn test_response(headers: &str, body: Vec<u8>) -> Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let headers = headers.to_string();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });

        Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap()
    }

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
            (
                "https://immich.example",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/api",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/api/",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/custom/path",
                "https://immich.example/custom/path/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example:8443",
                "https://immich.example:8443/api/assets/bulk-upload-check",
            ),
        ];

        for (server_url, expected_endpoint) in cases {
            let client = ImmichClient::new(server_url.to_string(), "key".to_string())
                .expect("HTTPS URL should be accepted");
            assert_eq!(
                client.endpoint_url("/assets/bulk-upload-check").as_str(),
                expected_endpoint
            );
        }
    }

    #[test]
    fn calculates_all_upload_and_audit_checksums_in_one_pass() {
        let path = temporary_file(b"abc");
        let checksums = calculate_checksums(path.to_str().unwrap()).unwrap();
        // SHA-1 is stored in standard base64 for Immich; MD5/SHA-256 are lowercase hex.
        assert_eq!(checksums.sha1_base64, "qZk+NkcGgWq6PiVxeFDCbJzQ2J0=");
        assert_eq!(checksums.md5_hex, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            checksums.sha256_hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn mock_immich_handles_bulk_check_and_upload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(2);

        tokio::spawn(async move {
            for response in [
                r#"{"results":[{"id":"already-there","action":"reject","reason":"duplicate"},{"id":"new-file","action":"accept"}]}"#,
                r#"{"id":"uploaded-asset"}"#,
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut stream).await;
                requests_tx.send(request).await.unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            response.len(),
                            response
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });

        // Direct construction keeps the production HTTPS-only constructor intact while the
        // in-process mock uses plain HTTP.
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let existing = client
            .check_assets_exist(vec!["already-there".to_string(), "new-file".to_string()])
            .await
            .unwrap();
        assert_eq!(existing, vec!["already-there"]);

        let asset_path = temporary_file(b"image data");
        assert_eq!(
            client
                .upload_asset(asset_path.to_str().unwrap(), "new-file")
                .await
                .unwrap(),
            "uploaded-asset"
        );
        std::fs::remove_file(asset_path).unwrap();

        // Audit reporting is best effort: an audit failure must not prevent a
        // completed upload, and its UI-facing reporter still receives the error.
        let mut audit_ui_messages = Vec::new();
        report_audit(Err(AuditError::OversizedRecord), |message| {
            audit_ui_messages.push(message.to_string())
        });
        assert_eq!(audit_ui_messages.len(), 1);
        assert!(audit_ui_messages[0].contains("Audit logging failed"));

        let bulk_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(bulk_request.starts_with("POST /api/assets/bulk-upload-check HTTP/1.1"));
        assert!(bulk_request.contains("x-api-key: test-key"));
        assert!(bulk_request.contains(r#""id":"already-there""#));
        assert!(bulk_request.contains(r#""checksum":"new-file""#));

        let upload_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(upload_request.starts_with("POST /api/assets HTTP/1.1"));
        assert!(upload_request.contains("x-immich-checksum: new-file"));
        assert!(upload_request.contains("name=\"assetData\""));
    }

    #[tokio::test]
    async fn validates_login_with_current_user_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            requests_tx.send(request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        client.validate_connection().await.unwrap();

        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.starts_with("GET /api/users/me HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert!(!request.contains("bulk-upload-check"));
    }

    #[tokio::test]
    async fn mock_immich_rejects_malformed_bulk_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let error = client
            .check_assets_exist(vec!["file".to_string()])
            .await
            .unwrap_err();
        assert!(error.is_retryable());
        assert!(error.to_string().contains("missing results array"));
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

        let unsupported_format = json!({
            "results": [
                { "id": "first", "action": "reject", "reason": "unsupported-format" },
                { "id": "second", "action": "accept" }
            ]
        });
        assert!(parse_bulk_check_response(&unsupported_format, &hashes)
            .unwrap_err()
            .contains("unsupported format"));
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
    fn authentication_errors_are_not_retryable() {
        assert!(!bulk_check_status_error(StatusCode::UNAUTHORIZED).is_retryable());
        assert!(!bulk_check_status_error(StatusCode::FORBIDDEN).is_retryable());
        assert!(bulk_check_status_error(StatusCode::INTERNAL_SERVER_ERROR).is_retryable());
    }

    #[test]
    fn sidecar_size_limit_allows_boundary_and_rejects_larger_files() {
        assert!(validate_sidecar_size(MAX_SIDECAR_BYTES).is_ok());
        let error = validate_sidecar_size(MAX_SIDECAR_BYTES + 1).unwrap_err();
        assert!(error.contains("exceeds"));
        assert!(error.contains(&(MAX_SIDECAR_BYTES + 1).to_string()));
    }

    #[tokio::test]
    async fn response_body_limit_accepts_exact_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
            b"test".to_vec(),
        )
        .await;

        assert_eq!(
            read_response_body_limited(response, 4).await.unwrap(),
            b"test"
        );
    }

    #[tokio::test]
    async fn response_body_limit_rejects_oversized_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
            b"tests".to_vec(),
        )
        .await;

        assert!(read_response_body_limited(response, 4)
            .await
            .unwrap_err()
            .contains("exceeds"));
    }

    #[tokio::test]
    async fn response_body_limit_rejects_oversized_body_without_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            b"tests".to_vec(),
        )
        .await;

        assert!(read_response_body_limited(response, 4)
            .await
            .unwrap_err()
            .contains("exceeds"));
    }

    #[tokio::test]
    async fn error_body_is_capped_and_marked_as_truncated() {
        let body = vec![b'x'; MAX_ERROR_RESPONSE_BYTES + 1];
        let response = test_response(
            &format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            body,
        )
        .await;

        let detail = read_error_body_capped(response, MAX_ERROR_RESPONSE_BYTES).await;
        assert!(detail.ends_with("... [truncated]"));
        assert_eq!(
            detail.len(),
            MAX_ERROR_RESPONSE_BYTES + ": ".len() + "... [truncated]".len()
        );
    }
}
