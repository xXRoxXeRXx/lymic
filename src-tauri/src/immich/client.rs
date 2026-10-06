use super::bulk_check::{bulk_check_response_error, parse_bulk_check_response, BulkCheckError};
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use super::upload::parse_upload_response;
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::json;
use url::Url;

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
    pub(crate) fn new(server_url: String, api_key: String) -> Result<Self, String> {
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

    pub(super) fn endpoint_url(&self, path: &str) -> Url {
        self.server_url
            .join(path.trim_start_matches('/'))
            .expect("validated base URL must support relative endpoint paths")
    }

    pub(crate) async fn validate_connection(&self) -> Result<(), String> {
        self.current_user().await.map(|_| ())
    }

    pub(crate) async fn current_user(&self) -> Result<String, String> {
        let response = self
            .client
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
            .map_err(|_| BulkCheckError::Retryable("Check request failed".to_string()))?;
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
    ) -> Result<String, String> {
        let form = super::upload::upload_form(path, live_photo_path).await?;
        let response: Response = self
            .client
            .post(self.endpoint_url("assets"))
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", precomputed_hash)
            .timeout(std::time::Duration::from_secs(300))
            .multipart(form)
            .send()
            .await
            .map_err(|_| "Network error during upload".to_string())?;
        if response.status().is_success() {
            parse_upload_response(
                &read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
            )
        } else {
            Err(format!(
                "Upload failed: {}",
                response_status_error(response).await
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ImmichClient;
    use reqwest::Client;
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

    fn temporary_file(contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-immich-client-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
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
            let client = ImmichClient::new(server_url.to_string(), "key".to_string()).unwrap();
            assert_eq!(
                client.endpoint_url("/assets/bulk-upload-check").as_str(),
                expected_endpoint
            );
        }
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
            client: Client::new(),
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
                .upload_asset_with_live_photo(asset_path.to_str().unwrap(), "new-file", None)
                .await
                .unwrap(),
            "uploaded-asset"
        );
        std::fs::remove_file(asset_path).unwrap();
        let bulk_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(bulk_request.starts_with("POST /api/assets/bulk-upload-check HTTP/1.1"));
        assert!(bulk_request.contains("x-api-key: test-key"));
        assert!(bulk_request.contains(r#""id":"already-there""#));
        assert!(bulk_request.contains(r#""checksum":"new-file""#));
        let upload_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(upload_request.starts_with("POST /api/assets HTTP/1.1"));
        assert!(upload_request.contains("x-immich-checksum: new-file"));
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
                    Some(video.to_str().unwrap())
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
