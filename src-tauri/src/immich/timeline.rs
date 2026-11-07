use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBucket {
    pub time_bucket: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineAsset {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
struct TimelineAssetResponse {
    id: String,
    #[serde(rename = "exifInfo", alias = "exif")]
    exif: TimelineAssetExif,
}

#[derive(Deserialize)]
struct TimelineAssetExif {
    #[serde(rename = "imageWidth", alias = "width")]
    width: u32,
    #[serde(rename = "imageHeight", alias = "height")]
    height: u32,
}

impl ImmichClient {
    pub async fn get_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        let response = self
            .client
            .get(self.endpoint_url("timeline/buckets"))
            .query(&[("size", "MONTH")])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch timeline buckets: {error}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch timeline buckets: {}",
                response_status_error(response).await?
            ));
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let mut buckets: Vec<TimelineBucket> = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid timeline buckets response: {error}"))?;
        buckets.sort_by(|a, b| b.time_bucket.cmp(&a.time_bucket));
        Ok(buckets)
    }

    pub async fn get_timeline_bucket_assets(
        &self,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        let response = self
            .client
            .get(self.endpoint_url("timeline/bucket"))
            .query(&[("size", "MONTH"), ("timeBucket", time_bucket)])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch timeline bucket: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch timeline bucket: {}",
                response_status_error(response).await?
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let assets: Vec<TimelineAssetResponse> = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid timeline bucket response: {error}"))?;
        Ok(assets
            .into_iter()
            .map(|asset| TimelineAsset {
                id: asset.id,
                width: asset.exif.width,
                height: asset.exif.height,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::ImmichClient;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn fetches_month_assets_with_ids_and_dimensions() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with(
                "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z HTTP/1.1"
            ));
            assert!(request.contains("x-api-key: test-key"));
            let body =
                b"[{\"id\":\"asset-1\",\"exifInfo\":{\"imageWidth\":640,\"imageHeight\":480}}]";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
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
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let assets = client
            .get_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, "asset-1");
        assert_eq!(assets[0].width, 640);
        assert_eq!(assets[0].height, 480);
        server.await.unwrap();
    }
}
