use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: super::ids::AlbumId,
    pub album_name: String,
}

impl ImmichClient {
    pub async fn get_albums(&self) -> Result<Vec<Album>, String> {
        let response = self
            .client
            .get(self.endpoint_url("albums"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch albums: {}", error))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch albums: {}",
                response_status_error(response).await?
            ));
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let mut albums: Vec<Album> = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid albums response: {}", error))?;

        albums.sort_by_key(|a| a.album_name.to_lowercase());
        Ok(albums)
    }

    pub async fn create_album(&self, album_name: &str) -> Result<Album, String> {
        let trimmed_name = album_name.trim();
        if trimmed_name.is_empty() {
            return Err("Album name cannot be empty".to_string());
        }

        let response = self
            .client
            .post(self.endpoint_url("albums"))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "albumName": trimmed_name }))
            .send()
            .await
            .map_err(|error| format!("Failed to create album: {}", error))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to create album: {}",
                response_status_error(response).await?
            ));
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid create album response: {}", error))
    }

    pub async fn add_assets_to_album(
        &self,
        album_id: &super::ids::AlbumId,
        asset_ids: &[super::ids::RemoteAssetId],
    ) -> Result<(), String> {
        Self::validate_path_segment(album_id.as_str(), "album ID")?;
        if asset_ids.is_empty() {
            return Ok(());
        }

        let endpoint = format!("albums/{}/assets", album_id);
        let response = self
            .client
            .put(self.endpoint_url(&endpoint))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "ids": asset_ids }))
            .send()
            .await
            .map_err(|error| format!("Failed to assign assets to album: {}", error))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to assign assets to album: {}",
                response_status_error(response).await?
            ));
        }

        Ok(())
    }

    pub async fn search_asset_by_checksum(
        &self,
        checksum: &str,
    ) -> Result<Option<super::ids::RemoteAssetId>, String> {
        let response = self
            .client
            .post(self.endpoint_url("search/metadata"))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "checksum": checksum }))
            .send()
            .await
            .map_err(|error| format!("Checksum search request failed: {}", error))?;

        if !response.status().is_success() {
            return Err(format!(
                "Checksum search failed: {}",
                response_status_error(response).await?
            ));
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let data: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid checksum search response: {}", error))?;

        let items = if let Some(items) = data
            .get("assets")
            .and_then(|a| a.get("items"))
            .and_then(serde_json::Value::as_array)
        {
            items.as_slice()
        } else if let Some(items) = data.get("items").and_then(serde_json::Value::as_array) {
            items.as_slice()
        } else if let Some(arr) = data.as_array() {
            arr.as_slice()
        } else {
            &[]
        };

        let ids: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
            .collect();

        match ids.len() {
            0 => Ok(None),
            1 => Ok(Some(super::ids::RemoteAssetId::new(ids[0]))),
            count => Err(format!(
                "Checksum '{}' matched {} assets remotely; cannot unambiguously resolve target album asset",
                checksum, count
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::{AlbumId, RemoteAssetId};
    use reqwest::Client;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
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
        String::from_utf8_lossy(&request).to_string()
    }

    #[tokio::test]
    async fn get_albums_fetches_and_sorts_alphabetically() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_http_request(&mut stream).await;
            assert!(req.starts_with("GET /api/albums HTTP/1.1"));
            assert!(req.contains("x-api-key: test-key"));
            let response = r#"[{"id":"2","albumName":"Zebra"},{"id":"1","albumName":"apple"},{"id":"3","albumName":"Banana"}]"#;
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
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let albums = client.get_albums().await.unwrap();
        assert_eq!(
            albums,
            vec![
                Album {
                    id: AlbumId::new("1"),
                    album_name: "apple".to_string(),
                },
                Album {
                    id: AlbumId::new("3"),
                    album_name: "Banana".to_string(),
                },
                Album {
                    id: AlbumId::new("2"),
                    album_name: "Zebra".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn create_album_sends_album_name_and_returns_created_album() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_http_request(&mut stream).await;
            assert!(req.starts_with("POST /api/albums HTTP/1.1"));
            assert!(req.contains(r#"{"albumName":"Vacation"}"#));
            let response = r#"{"id":"new-album-id","albumName":"Vacation"}"#;
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
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let created = client.create_album("  Vacation  ").await.unwrap();
        assert_eq!(
            created,
            Album {
                id: AlbumId::new("new-album-id"),
                album_name: "Vacation".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn create_album_rejects_empty_or_whitespace_name_locally() {
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse("http://127.0.0.1:9999/api/").unwrap(),
            api_key: "test-key".to_string(),
        };

        assert!(client.create_album("").await.is_err());
        assert!(client.create_album("   \t  ").await.is_err());
    }

    #[tokio::test]
    async fn add_assets_to_album_sends_ids_to_album_assets_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_http_request(&mut stream).await;
            assert!(req.starts_with("PUT /api/albums/album-123/assets HTTP/1.1"));
            assert!(req.contains(r#"{"ids":["asset-1","asset-2"]}"#));
            let response = r#"[{"id":"asset-1","success":true},{"id":"asset-2","success":true}]"#;
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
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let ids = vec![RemoteAssetId::new("asset-1"), RemoteAssetId::new("asset-2")];
        assert!(client
            .add_assets_to_album(&AlbumId::new("album-123"), &ids)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn add_assets_to_album_rejects_unsafe_album_ids_before_requesting() {
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse("http://127.0.0.1:9999/api/").unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_ids = vec![RemoteAssetId::new("asset-1")];

        for album_id in [
            "",
            "   ",
            "../other",
            "album..id",
            "album/id",
            "album\\id",
            "album?x",
            "album#x",
            "album\tid",
        ] {
            assert_eq!(
                client
                    .add_assets_to_album(&AlbumId::new(album_id), &asset_ids)
                    .await
                    .unwrap_err(),
                "Invalid album ID for API path"
            );
        }
    }

    #[tokio::test]
    async fn add_assets_to_album_fails_loudly_on_server_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            let response = r#"{"message":"Album not found"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.len(),
                        response
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let ids = vec![RemoteAssetId::new("asset-1")];
        let err = client
            .add_assets_to_album(&AlbumId::new("deleted-album"), &ids)
            .await
            .unwrap_err();
        assert!(err.contains("Failed to assign assets to album"));
    }

    #[tokio::test]
    async fn search_asset_by_checksum_resolves_unique_asset_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let req = read_http_request(&mut stream).await;
            assert!(req.starts_with("POST /api/search/metadata HTTP/1.1"));
            assert!(req.contains(r#"{"checksum":"sha1-hash"}"#));
            let response = r#"{"assets":{"items":[{"id":"resolved-remote-id"}]}}"#;
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
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let resolved = client.search_asset_by_checksum("sha1-hash").await.unwrap();
        assert_eq!(resolved, Some(RemoteAssetId::new("resolved-remote-id")));
    }

    #[tokio::test]
    async fn search_asset_by_checksum_rejects_ambiguous_matches() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            let response = r#"{"assets":{"items":[{"id":"id-1"},{"id":"id-2"}]}}"#;
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
        });

        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        let err = client
            .search_asset_by_checksum("sha1-hash")
            .await
            .unwrap_err();
        assert!(err.contains("cannot unambiguously resolve target album asset"));
    }
}
