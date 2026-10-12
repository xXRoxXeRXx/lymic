use super::bulk_check::{bulk_check_response_error, parse_bulk_check_response, BulkCheckError};
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use super::upload::{parse_upload_response, upload_form, SidecarDirectoryCache};
use crate::sync::progress::TransferStatsTracker;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Response,
};
use serde::Deserialize;
use serde_json::json;
use url::{Host, Url};

const UPLOAD_TIMEOUT_BASE_SECONDS: u64 = 5 * 60;
const UPLOAD_TIMEOUT_SECONDS_PER_GIB: u64 = 2 * 60 * 60;
const GIB_BYTES: u128 = 1024 * 1024 * 1024;
pub(crate) const LOCAL_HTTP_NOT_ALLOWED_ERROR: &str = "LOCAL_HTTP_NOT_ALLOWED";

fn upload_timeout(total_file_bytes: u64) -> std::time::Duration {
    let proportional_seconds = (u128::from(total_file_bytes)
        .saturating_mul(u128::from(UPLOAD_TIMEOUT_SECONDS_PER_GIB))
        .saturating_add(GIB_BYTES - 1))
        / GIB_BYTES;
    let total_seconds =
        u128::from(UPLOAD_TIMEOUT_BASE_SECONDS).saturating_add(proportional_seconds);
    std::time::Duration::from_secs(u64::try_from(total_seconds).unwrap_or(u64::MAX))
}

pub(crate) struct ImmichClient {
    pub(super) client: Client,
    pub(super) server_url: Url,
    pub(super) api_key: String,
}

#[derive(Deserialize)]
struct CurrentUserResponse {
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
}

impl ImmichClient {
    pub(crate) fn new(
        server_url: String,
        api_key: String,
        allow_local_http: bool,
        cloudflare_client_id: Option<String>,
        cloudflare_client_secret: Option<String>,
    ) -> Result<Self, String> {
        let mut base_url =
            Url::parse(server_url.trim()).map_err(|_| "Invalid server URL".to_string())?;
        match base_url.scheme() {
            "https" => {}
            "http" if allow_local_http && is_loopback_host(base_url.host()) => {}
            "http" => {
                return Err(
                    format!(
                        "{LOCAL_HTTP_NOT_ALLOWED_ERROR}: HTTP is allowed only for localhost and loopback addresses. All other servers require HTTPS."
                    ),
                )
            }
            _ => return Err("Only HTTPS server URLs are supported".to_string()),
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
        let default_headers = match (cloudflare_client_id, cloudflare_client_secret) {
            (None, None) => HeaderMap::new(),
            (Some(client_id), Some(client_secret)) => {
                let mut headers = HeaderMap::new();
                let client_id = HeaderValue::from_str(&client_id)
                    .map_err(|_| "Invalid Cloudflare Access client ID".to_string())?;
                let mut client_secret = HeaderValue::from_str(&client_secret)
                    .map_err(|_| "Invalid Cloudflare Access client secret".to_string())?;
                client_secret.set_sensitive(true);
                headers.insert(HeaderName::from_static("cf-access-client-id"), client_id);
                headers.insert(
                    HeaderName::from_static("cf-access-client-secret"),
                    client_secret,
                );
                headers
            }
            _ => {
                return Err(
                    "Cloudflare Access client ID and client secret must both be provided"
                        .to_string(),
                )
            }
        };
        Ok(Self {
            // Reject redirects so an HTTPS endpoint cannot downgrade an API-key request to HTTP.
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .default_headers(default_headers)
                .build()
                .map_err(|_| "Failed to initialize HTTP client".to_string())?,
            server_url: base_url,
            api_key,
        })
    }

    pub(super) fn endpoint_url(&self, path: &str) -> Url {
        self.server_url
            .join(path.trim_start_matches('/'))
            .expect("validated base URL must support relative endpoint paths")
    }

    pub(crate) async fn validate_connection(&self) -> Result<(), String> {
        self.current_user().await.map(|_| ())
    }

    pub(crate) fn uses_http(&self) -> bool {
        self.server_url.scheme() == "http"
    }

    pub(crate) async fn current_user(&self) -> Result<String, String> {
        let response = self
            .client
            .get(self.endpoint_url("users/me"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Connection failed: {}", error))?;
        if !response.status().is_success() {
            return Err(format!(
                "Server returned error: {}",
                response_status_error(response).await?
            ));
        }
        let user: CurrentUserResponse = response
            .json()
            .await
            .map_err(|_| "Invalid current-user response".to_string())?;
        let display_name = if user.name.trim().is_empty() {
            user.email.trim()
        } else {
            user.name.trim()
        };
        if display_name.is_empty() {
            Err("Current user response did not include a name".to_string())
        } else {
            Ok(display_name.to_string())
        }
    }

    pub(crate) async fn check_assets_exist(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<String>, BulkCheckError> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let assets: Vec<serde_json::Value> = hashes
            .iter()
            .map(|hash| json!({ "id": hash, "checksum": hash }))
            .collect();
        let response = self
            .client
            .post(self.endpoint_url("assets/bulk-upload-check"))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "assets": assets }))
            .send()
            .await
            .map_err(|error| {
                BulkCheckError::Retryable(format!("Check request failed: {}", error))
            })?;
        if !response.status().is_success() {
            return Err(bulk_check_response_error(response).await);
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES)
            .await
            .map_err(|error| {
                BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", error))
            })?;
        let data = serde_json::from_slice(&body).map_err(|error| {
            BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", error))
        })?;
        parse_bulk_check_response(&data, &hashes).map_err(BulkCheckError::Retryable)
    }

    pub(crate) async fn upload_asset_with_live_photo(
        &self,
        path: &str,
        precomputed_hash: &str,
        live_photo_path: Option<&str>,
        transfer_tracker: Option<TransferStatsTracker>,
        sidecar_cache: SidecarDirectoryCache,
    ) -> Result<String, String> {
        let payload = upload_form(path, live_photo_path, transfer_tracker, sidecar_cache).await?;
        self.post_upload_form(
            payload.form,
            precomputed_hash,
            upload_timeout(payload.total_file_bytes),
        )
        .await
    }

    async fn post_upload_form(
        &self,
        form: reqwest::multipart::Form,
        precomputed_hash: &str,
        timeout: std::time::Duration,
    ) -> Result<String, String> {
        let response: Response = self
            .client
            .post(self.endpoint_url("assets"))
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", precomputed_hash)
            .timeout(timeout)
            .multipart(form)
            .send()
            .await
            .map_err(|error| format!("Network error during upload: {}", error))?;
        if response.status().is_success() {
            parse_upload_response(
                &read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
            )
        } else {
            Err(format!(
                "Upload failed: {}",
                response_status_error(response).await?
            ))
        }
    }
}

fn is_loopback_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        upload_timeout, ImmichClient, SidecarDirectoryCache, GIB_BYTES,
        LOCAL_HTTP_NOT_ALLOWED_ERROR, UPLOAD_TIMEOUT_BASE_SECONDS,
    };
    use reqwest::Client;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

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

    async fn read_http_headers(stream: &mut tokio::net::TcpStream) {
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request");
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                return;
            }
        }
    }

    fn temporary_file(contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-immich-client-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            Uuid::new_v4(),
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn cloudflare_test_client() -> Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("cf-access-client-id", "client-id".parse().unwrap());
        headers.insert("cf-access-client-secret", "client-secret".parse().unwrap());
        Client::builder().default_headers(headers).build().unwrap()
    }

    #[test]
    fn rejects_insecure_or_sensitive_server_urls() {
        for server_url in [
            "http://immich.example",
            "http://localhost",
            "https://user:password@immich.example",
            "https://immich.example/?token=secret",
            "https://immich.example/#fragment",
        ] {
            assert!(ImmichClient::new(
                server_url.to_string(),
                "key".to_string(),
                false,
                None,
                None
            )
            .is_err());
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
            let client =
                ImmichClient::new(server_url.to_string(), "key".to_string(), false, None, None)
                    .unwrap();
            assert_eq!(
                client.endpoint_url("/assets/bulk-upload-check").as_str(),
                expected_endpoint
            );
        }
    }

    #[test]
    fn rejects_incomplete_or_invalid_cloudflare_access_headers_locally() {
        assert!(ImmichClient::new(
            "https://immich.example".to_string(),
            "key".to_string(),
            false,
            Some("client-id".to_string()),
            None,
        )
        .is_err());
        assert!(ImmichClient::new(
            "https://immich.example".to_string(),
            "key".to_string(),
            false,
            Some("bad\nclient-id".to_string()),
            Some("client-secret".to_string()),
        )
        .is_err());
    }

    #[test]
    fn accepts_opted_in_http_only_for_loopback_hosts() {
        for server_url in [
            "http://localhost",
            "http://127.0.0.1",
            "http://127.255.255.255",
            "http://[::1]",
        ] {
            assert!(
                ImmichClient::new(server_url.to_string(), "key".to_string(), true, None, None)
                    .is_ok()
            );
        }

        for server_url in [
            "http://10.0.0.1",
            "http://192.168.1.1",
            "http://172.16.0.1",
            "http://immich.local",
            "http://immich.example",
            "ftp://localhost",
            "http://localhost/?token=secret",
            "http://localhost/#fragment",
            "http://user:password@localhost",
        ] {
            assert!(
                ImmichClient::new(server_url.to_string(), "key".to_string(), true, None, None)
                    .is_err()
            );
        }
    }

    #[test]
    fn local_http_rejection_uses_a_stable_error_code() {
        let error = ImmichClient::new(
            "http://immich.example".to_string(),
            "key".to_string(),
            true,
            None,
            None,
        )
        .err()
        .unwrap();

        assert!(error.starts_with(LOCAL_HTTP_NOT_ALLOWED_ERROR));
    }

    #[test]
    fn upload_timeout_scales_with_file_payload_size() {
        assert_eq!(
            upload_timeout(0),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS)
        );
        assert_eq!(
            upload_timeout(1),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 1)
        );
        assert_eq!(
            upload_timeout(GIB_BYTES as u64),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 2 * 60 * 60)
        );
        assert_eq!(
            upload_timeout(GIB_BYTES as u64 + 1),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 2 * 60 * 60 + 1)
        );
        assert!(upload_timeout(u64::MAX) > upload_timeout(GIB_BYTES as u64));
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
                requests_tx
                    .send(read_http_request(&mut stream).await)
                    .await
                    .unwrap();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
            }
        });
        let client = ImmichClient {
            client: cloudflare_test_client(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        assert_eq!(
            client
                .check_assets_exist(vec!["already-there".to_string(), "new-file".to_string()])
                .await
                .unwrap(),
            vec!["already-there"]
        );
        let asset_path = temporary_file(b"image data");
        assert_eq!(
            client
                .upload_asset_with_live_photo(
                    asset_path.to_str().unwrap(),
                    "new-file",
                    None,
                    None,
                    SidecarDirectoryCache::new(),
                )
                .await
                .unwrap(),
            "uploaded-asset"
        );
        std::fs::remove_file(asset_path).unwrap();
        let bulk_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(bulk_request.starts_with("POST /api/assets/bulk-upload-check HTTP/1.1"));
        assert!(bulk_request.contains("x-api-key: test-key"));
        assert!(bulk_request.contains("cf-access-client-id: client-id"));
        assert!(bulk_request.contains("cf-access-client-secret: client-secret"));
        assert!(bulk_request.contains(r#""id":"already-there""#));
        assert!(bulk_request.contains(r#""checksum":"new-file""#));
        let upload_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(upload_request.starts_with("POST /api/assets HTTP/1.1"));
        assert!(upload_request.contains("x-immich-checksum: new-file"));
        assert!(upload_request.contains("cf-access-client-id: client-id"));
        assert!(upload_request.contains("cf-access-client-secret: client-secret"));
        assert!(upload_request.contains("name=\"assetData\""));
        assert!(!upload_request.contains("name=\"livePhotoData\""));
    }

    #[tokio::test]
    async fn pair_upload_streams_image_and_live_photo_parts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"id\":\"paired\"}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let image = temporary_file(b"image data");
        let video = temporary_file(b"video data");
        let sidecar = image.with_extension("xmp");
        std::fs::write(&sidecar, b"sidecar data").unwrap();
        assert_eq!(
            client
                .upload_asset_with_live_photo(
                    image.to_str().unwrap(),
                    "image-hash",
                    Some(video.to_str().unwrap()),
                    None,
                    SidecarDirectoryCache::new(),
                )
                .await
                .unwrap(),
            "paired"
        );
        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.contains("x-immich-checksum: image-hash"));
        assert!(request.contains("name=\"assetData\""));
        assert!(request.contains("name=\"livePhotoData\""));
        assert!(request.contains("name=\"sidecarData\""));
        assert!(request.contains(image.file_name().unwrap().to_str().unwrap()));
        assert!(request.contains(video.file_name().unwrap().to_str().unwrap()));
        assert!(request.contains(sidecar.file_name().unwrap().to_str().unwrap()));
        std::fs::remove_file(image).unwrap();
        std::fs::remove_file(video).unwrap();
        std::fs::remove_file(sidecar).unwrap();
    }

    #[tokio::test]
    async fn upload_network_error_includes_reqwest_cause() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let error = client
            .upload_asset_with_live_photo(
                asset_path.to_str().unwrap(),
                "file-hash",
                None,
                None,
                SidecarDirectoryCache::new(),
            )
            .await
            .unwrap_err();
        std::fs::remove_file(asset_path).unwrap();

        let prefix = "Network error during upload: ";
        assert!(error.starts_with(prefix));
        assert!(error.len() > prefix.len());
    }

    #[tokio::test]
    async fn upload_request_uses_injected_timeout_when_server_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_headers(&mut stream).await;
            std::future::pending::<()>().await;
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let payload = super::upload_form(
            asset_path.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        let error = client
            .post_upload_form(
                payload.form,
                "file-hash",
                std::time::Duration::from_millis(50),
            )
            .await
            .unwrap_err();
        std::fs::remove_file(asset_path).unwrap();

        assert!(error.starts_with("Network error during upload: "));
    }

    #[tokio::test]
    async fn upload_request_succeeds_immediately_with_large_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"id\":\"large\"}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let payload = super::upload_form(
            asset_path.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        let response = client
            .post_upload_form(
                payload.form,
                "file-hash",
                upload_timeout(10 * GIB_BYTES as u64),
            )
            .await
            .unwrap();
        std::fs::remove_file(asset_path).unwrap();

        assert_eq!(response, "large");
    }

    #[tokio::test]
    async fn validates_login_with_current_user_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 31\r\nConnection: close\r\n\r\n{\"name\":\"Test User\",\"email\":\"\"}").await.unwrap();
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
        assert!(!request.contains("cf-access-client-id"));
        assert!(!request.contains("cf-access-client-secret"));
        assert!(!request.contains("bulk-upload-check"));
    }

    #[tokio::test]
    async fn mock_immich_rejects_malformed_bulk_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
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
}
