use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::StatusCode;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonTimelinePage {
    pub assets: Vec<TimelineAsset>,
    pub has_next_page: bool,
}

fn recognized_people_access_error(status: StatusCode, resource: &str) -> String {
    match status {
        StatusCode::UNAUTHORIZED => format!("recognized_people_authentication_required:{resource}"),
        StatusCode::FORBIDDEN => format!("recognized_people_permission_denied:{resource}:configure an API key with person.read and asset.read permissions"),
        _ => format!("recognized_people_unavailable:{resource}"),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TimelineBucketPayload {
    Sequence(Vec<TimelineAssetResponse>),
    Columnar(ColumnarTimelineBucketResponse),
}

#[derive(Deserialize)]
struct TimelineAssetResponse {
    id: String,
    #[serde(rename = "type", default = "default_image_type")]
    asset_type: String,
    #[serde(rename = "exifInfo", alias = "exif", default)]
    exif: TimelineAssetExif,
    #[serde(default, deserialize_with = "deserialize_duration")]
    duration: Option<u64>,
}

#[derive(Default, Deserialize)]
struct TimelineAssetExif {
    #[serde(
        rename = "imageWidth",
        alias = "exifImageWidth",
        alias = "width",
        default
    )]
    width: u32,
    #[serde(
        rename = "imageHeight",
        alias = "exifImageHeight",
        alias = "height",
        default
    )]
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

#[derive(Deserialize)]
struct ColumnarTimelineBucketResponse {
    id: Vec<String>,
    #[serde(rename = "isImage", default)]
    is_image: Vec<bool>,
    #[serde(default)]
    ratio: Vec<Option<f64>>,
    #[serde(default)]
    duration: Vec<Option<serde_json::Value>>,
    #[serde(rename = "fileCreatedAt", default)]
    file_created_at: Vec<Option<String>>,
    #[serde(rename = "createdAt", default)]
    created_at: Vec<Option<String>>,
}

fn default_image_type() -> String {
    "IMAGE".to_string()
}

fn parse_duration_value(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|ms| (ms / 1000.0).round() as u64),
        serde_json::Value::String(value) => {
            if value.contains(':') {
                let mut total_seconds = 0.0_f64;
                for part in value.split(':') {
                    let parsed: f64 = part.parse().ok()?;
                    total_seconds = total_seconds * 60.0 + parsed;
                }
                (total_seconds.is_finite() && total_seconds >= 0.0)
                    .then_some(total_seconds.round() as u64)
            } else {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(|ms| (ms / 1000.0).round() as u64)
            }
        }
        _ => None,
    }
}

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(parse_duration_value))
}

fn dimensions_from_ratio(ratio: Option<f64>) -> (u32, u32) {
    match ratio {
        Some(value) if value.is_finite() && value > 0.0 => {
            let height = 1000_u32;
            let width = (value * f64::from(height))
                .round()
                .clamp(1.0, f64::from(u32::MAX)) as u32;
            (width, height)
        }
        _ => (0, 0),
    }
}

impl ImmichClient {
    pub async fn get_person_timeline_assets(
        &self,
        person_id: &str,
        page: u32,
        size: u32,
    ) -> Result<PersonTimelinePage, String> {
        Self::validate_path_segment(person_id, "person ID")?;
        let response = self
            .client
            .post(self.endpoint_url("search/metadata"))
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({ "personIds": [person_id], "size": size, "page": page }))
            .send()
            .await
            .map_err(|_| "Unable to load recognized person assets".to_string())?;
        if !response.status().is_success() {
            return Err(recognized_people_access_error(
                response.status(),
                "timeline",
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| "Invalid recognized person assets response".to_string())?;
        let response_assets = value.get("assets");
        let assets = response_assets
            .and_then(|assets| assets.get("items"))
            .or(response_assets)
            .unwrap_or(&value);
        let assets: Vec<TimelineAssetResponse> = serde_json::from_value(assets.clone())
            .map_err(|_| "Invalid recognized person assets response".to_string())?;
        let has_next_page = response_assets
            .and_then(|assets| {
                assets
                    .get("hasNextPage")
                    .and_then(serde_json::Value::as_bool)
            })
            .or_else(|| {
                response_assets
                    .and_then(|assets| assets.get("nextPage"))
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or_else(|| {
                response_assets
                    .and_then(|assets| assets.get("total").and_then(serde_json::Value::as_u64))
                    .is_some_and(|total| u64::from(page) * u64::from(size) < total)
            });
        Ok(PersonTimelinePage {
            assets: assets
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
                .collect(),
            has_next_page,
        })
    }

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
        let payload: TimelineBucketPayload = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid timeline bucket response: {error}"))?;
        Ok(match payload {
            TimelineBucketPayload::Sequence(assets) => assets
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
                .collect(),
            TimelineBucketPayload::Columnar(columnar) => columnar
                .id
                .into_iter()
                .enumerate()
                .map(|(index, id)| {
                    let (width, height) =
                        dimensions_from_ratio(columnar.ratio.get(index).copied().flatten());
                    let is_image = columnar.is_image.get(index).copied().unwrap_or(true);
                    let taken_at = columnar
                        .file_created_at
                        .get(index)
                        .cloned()
                        .flatten()
                        .or_else(|| columnar.created_at.get(index).cloned().flatten());
                    let duration = columnar
                        .duration
                        .get(index)
                        .and_then(|value| value.as_ref())
                        .and_then(parse_duration_value);
                    TimelineAsset {
                        id,
                        width,
                        height,
                        r#type: if is_image {
                            "IMAGE".to_string()
                        } else {
                            "VIDEO".to_string()
                        },
                        taken_at,
                        camera_make: None,
                        camera_model: None,
                        file_size: None,
                        duration,
                    }
                })
                .collect(),
        })
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
                b"[{\"id\":\"asset-1\",\"type\":\"VIDEO\",\"duration\":65000,\"exifInfo\":{\"imageWidth\":640,\"imageHeight\":480,\"dateTimeOriginal\":\"2025-03-01T12:30:00.000Z\",\"make\":\"Canon\",\"model\":\"EOS R6\",\"fileSizeInByte\":1234}}]";
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

    #[tokio::test]
    async fn fetches_buckets_with_api_key_and_parses_columnar_bucket_assets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                assert!(request.starts_with("GET /api/timeline/buckets?size=MONTH HTTP/1.1"));
                assert!(request.contains("x-api-key: test-key"));
                let buckets_body = b"[{\"timeBucket\":\"2026-11-01\",\"count\":2}]";
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            buckets_body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                stream.write_all(buckets_body).await.unwrap();
            }

            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request
                .starts_with("GET /api/timeline/bucket?size=MONTH&timeBucket=2026-11-01 HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            let assets_body = br#"{"id":["photo-1","video-1","video-2","video-3"],"isImage":[true,false,false,false],"ratio":[0.67,1.7778,1.7778,1.7778],"duration":[null,"0:01:05.000",2205,"5231"],"fileCreatedAt":["2026-11-04T19:47:41.29","2026-11-03T10:00:00.000Z","2026-11-02T10:00:00.000Z","2026-11-01T10:00:00.000Z"]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        assets_body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(assets_body).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let buckets = client.get_timeline_buckets().await.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].time_bucket, "2026-11-01");
        assert_eq!(buckets[0].count, 2);

        let assets = client
            .get_timeline_bucket_assets("2026-11-01")
            .await
            .unwrap();
        assert_eq!(assets.len(), 4);
        assert_eq!(assets[0].id, "photo-1");
        assert_eq!(assets[0].width, 670);
        assert_eq!(assets[0].height, 1000);
        assert_eq!(assets[0].r#type, "IMAGE");
        assert_eq!(
            assets[0].taken_at.as_deref(),
            Some("2026-11-04T19:47:41.29")
        );
        assert_eq!(assets[0].duration, None);

        assert_eq!(assets[1].id, "video-1");
        assert_eq!(assets[1].width, 1778);
        assert_eq!(assets[1].height, 1000);
        assert_eq!(assets[1].r#type, "VIDEO");
        assert_eq!(assets[1].duration, Some(65));

        assert_eq!(assets[2].id, "video-2");
        assert_eq!(assets[2].duration, Some(2));

        assert_eq!(assets[3].id, "video-3");
        assert_eq!(assets[3].duration, Some(5));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fetches_assets_filtered_to_a_recognized_person() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            assert!(request.contains("\"personIds\":[\"person-1\"]"));
            let body = br#"{"assets":{"items":[{"id":"asset-1","type":"IMAGE","exifInfo":{"imageWidth":640,"imageHeight":480,"dateTimeOriginal":"2025-03-01T12:30:00.000Z"}}]}}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
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
            .get_person_timeline_assets("person-1", 1, 250)
            .await
            .unwrap();

        assert_eq!(assets.assets.len(), 1);
        assert_eq!(assets.assets[0].id, "asset-1");
        assert!(!assets.has_next_page);
        assert_eq!(
            assets[0].taken_at.as_deref(),
            Some("2025-03-01T12:30:00.000Z")
        );
        server.await.unwrap();
    }
}
