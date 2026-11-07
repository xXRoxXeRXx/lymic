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
    pub r#type: String,
    pub taken_at: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub file_size: Option<u64>,
    pub duration: Option<u64>,
}

#[derive(Deserialize)]
struct TimelineAssetResponse {
    id: String,
    #[serde(rename = "type", default = "default_image_type")]
    asset_type: String,
    #[serde(rename = "exifInfo", alias = "exif")]
    exif: TimelineAssetExif,
    #[serde(default, deserialize_with = "deserialize_duration")]
    duration: Option<u64>,
}

#[derive(Deserialize)]
struct TimelineAssetExif {
    #[serde(rename = "imageWidth", alias = "width")]
    width: u32,
    #[serde(rename = "imageHeight", alias = "height")]
    height: u32,
    #[serde(rename = "dateTimeOriginal", default)]
    taken_at: Option<String>,
    #[serde(default)]
    make: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(rename = "fileSizeInByte", default)]
    file_size: Option<u64>,
}

fn default_image_type() -> String {
    "IMAGE".to_string()
}

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(value) => value.parse().ok(),
        _ => None,
    }))
}

impl ImmichClient {
    pub async fn get_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        let response = self
            .client
            .get(self.endpoint_url("timeline/buckets"))
            .query(&[("size", "MONTH")])
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
                r#type: asset.asset_type,
                taken_at: asset.exif.taken_at,
                camera_make: asset.exif.make,
                camera_model: asset.exif.model,
                file_size: asset.exif.file_size,
                duration: asset.duration,
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
                b"[{\"id\":\"asset-1\",\"type\":\"VIDEO\",\"duration\":65,\"exifInfo\":{\"imageWidth\":640,\"imageHeight\":480,\"dateTimeOriginal\":\"2025-03-01T12:30:00.000Z\",\"make\":\"Canon\",\"model\":\"EOS R6\",\"fileSizeInByte\":1234}}]";
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
        assert_eq!(assets[0].r#type, "VIDEO");
        assert_eq!(assets[0].duration, Some(65));
        assert_eq!(assets[0].camera_model.as_deref(), Some("EOS R6"));
        server.await.unwrap();
    }
}
