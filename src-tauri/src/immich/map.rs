use super::client::ImmichClient;
use super::response::read_response_body_limited;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

// 300,000 typical marker objects fit comfortably; unrelated JSON remains capped at 1 MiB.
const MAX_MAP_MARKER_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct MapMarker {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Deserialize)]
struct MarkerResponse {
    id: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

impl ImmichClient {
    pub async fn get_map_markers(&self) -> Result<Vec<MapMarker>, String> {
        let response = self
            .client
            .get(self.endpoint_url("map/markers"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|_| "map_unavailable".to_string())?;
        match response.status() {
            StatusCode::UNAUTHORIZED => return Err("map_authentication_required".into()),
            StatusCode::FORBIDDEN => return Err("map_permission_denied:configure an API key with map.read and asset.read permissions".into()),
            status if !status.is_success() => return Err("map_unavailable".into()),
            _ => {}
        }
        let body = read_response_body_limited(response, MAX_MAP_MARKER_RESPONSE_BYTES).await?;
        let markers: Vec<MarkerResponse> =
            serde_json::from_slice(&body).map_err(|_| "map_invalid_response".to_string())?;
        Ok(markers
            .into_iter()
            .filter_map(|marker| {
                let (lat, lon) = (marker.lat?, marker.lon?);
                (lat.is_finite()
                    && lon.is_finite()
                    && (-90.0..=90.0).contains(&lat)
                    && (-180.0..=180.0).contains(&lon))
                .then_some(MapMarker {
                    id: marker.id,
                    lat,
                    lon,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::immich::ImmichClient;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn retrieves_valid_map_markers_with_zero_coordinates() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /api/map/markers HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            let body = b"[{\"id\":\"zero\",\"lat\":0,\"lon\":0},{\"id\":\"invalid\",\"lat\":91,\"lon\":2},{\"id\":\"missing\",\"lat\":null,\"lon\":2}]";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".into(),
            true,
            None,
            None,
        )
        .unwrap();
        let markers = client.get_map_markers().await.unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].id, "zero");
        assert_eq!((markers[0].lat, markers[0].lon), (0.0, 0.0));
        server.await.unwrap();
    }
    async fn request_with_response(
        status: &str,
        headers: &str,
        body: &str,
    ) -> Result<Vec<super::MapMarker>, String> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\n{headers}\r\nConnection: close\r\n\r\n{body}");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "secret".into(),
            true,
            None,
            None,
        )
        .unwrap();
        client.get_map_markers().await
    }

    #[tokio::test]
    async fn reports_permission_denial_without_exposing_response_body() {
        let error = request_with_response("403 Forbidden", "Content-Length: 6", "secret")
            .await
            .unwrap_err();
        assert!(error.starts_with("map_permission_denied:"));
        assert!(!error.contains("secret"));
    }

    #[tokio::test]
    async fn rejects_malformed_and_oversized_marker_responses() {
        let malformed = request_with_response("200 OK", "Content-Length: 1", "{")
            .await
            .unwrap_err();
        assert_eq!(malformed, "map_invalid_response");
        let oversized = request_with_response("200 OK", "Content-Length: 134217729", "")
            .await
            .unwrap_err();
        assert!(oversized.contains("exceeds"));
    }
}
