use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use chrono::DateTime;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// A complete, locally indexed Album Timeline. Unlike the library timeline,
/// album metadata search has no bucket endpoint, so assets are retained here
/// after every search page has been collected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AlbumTimeline {
    pub buckets: Vec<TimelineBucket>,
    pub assets_by_bucket: BTreeMap<String, Vec<TimelineAsset>>,
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
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(rename = "exifInfo", alias = "exif", default)]
    exif: TimelineAssetExif,
    #[serde(rename = "localDateTime", default)]
    local_date_time: Option<String>,
    #[serde(rename = "fileCreatedAt", default)]
    file_created_at: Option<String>,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_duration")]
    duration: Option<u64>,
}

#[derive(Deserialize)]
struct MetadataSearchResponse {
    assets: MetadataSearchAssets,
}

#[derive(Deserialize)]
struct MetadataSearchAssets {
    #[serde(default)]
    items: Vec<TimelineAssetResponse>,
    #[serde(
        rename = "nextPage",
        default,
        deserialize_with = "deserialize_next_page"
    )]
    next_page: Option<u64>,
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

fn deserialize_next_page<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid nextPage number"))
            .map(Some),
        Some(serde_json::Value::String(text)) => text
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom("invalid nextPage token"))
            .map(Some),
        Some(_) => Err(serde::de::Error::custom("invalid nextPage value")),
    }
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
    pub async fn get_album_timeline(&self, album_id: &str) -> Result<AlbumTimeline, String> {
        const PAGE_SIZE: u64 = 1_000;
        let mut page = 1_u64;
        let mut assets = Vec::new();

        loop {
            let response = self
                .client
                .post(self.endpoint_url("search/metadata"))
                .header("x-api-key", &self.api_key)
                .json(&serde_json::json!({
                    "albumIds": [album_id],
                    "order": "desc",
                    "withExif": true,
                    "page": page,
                    "size": PAGE_SIZE,
                }))
                .send()
                .await
                .map_err(|error| format!("Failed to fetch album timeline: {error}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "Failed to fetch album timeline: {}",
                    response_status_error(response).await?
                ));
            }

            let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
            let payload: MetadataSearchResponse = serde_json::from_slice(&body)
                .map_err(|error| format!("Invalid album timeline response: {error}"))?;
            assets.extend(
                payload
                    .assets
                    .items
                    .into_iter()
                    .map(timeline_asset_from_response),
            );

            match payload.assets.next_page {
                Some(next_page) if next_page > page => page = next_page,
                Some(_) => return Err("Invalid album timeline pagination response".to_string()),
                None => break,
            }
        }

        Ok(group_album_timeline_assets(assets))
    }

    pub async fn get_person_timeline_buckets(
        &self,
        person_id: &str,
    ) -> Result<Vec<TimelineBucket>, String> {
        Self::validate_path_segment(person_id, "person ID")?;
        let response = self
            .client
            .get(self.endpoint_url("timeline/buckets"))
            .header("x-api-key", &self.api_key)
            .query(&[("size", "MONTH"), ("personId", person_id)])
            .send()
            .await
            .map_err(|_| "Unable to load recognized person timeline".to_string())?;
        if !response.status().is_success() {
            return Err(recognized_people_access_error(
                response.status(),
                "timeline",
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let mut buckets: Vec<TimelineBucket> = serde_json::from_slice(&body)
            .map_err(|_| "Invalid recognized person timeline response".to_string())?;
        buckets.sort_by(|a, b| b.time_bucket.cmp(&a.time_bucket));
        Ok(buckets)
    }

    pub async fn get_person_timeline_bucket_assets(
        &self,
        person_id: &str,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        Self::validate_path_segment(person_id, "person ID")?;
        let response = self
            .client
            .get(self.endpoint_url("timeline/bucket"))
            .query(&[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("personId", person_id),
            ])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|_| "Unable to load recognized person timeline assets".to_string())?;
        if !response.status().is_success() {
            return Err(recognized_people_access_error(
                response.status(),
                "timeline",
            ));
        }
        parse_timeline_bucket_assets(
            read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
        )
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
        parse_timeline_bucket_assets(
            read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
        )
    }
}

fn parse_timeline_bucket_assets(body: Vec<u8>) -> Result<Vec<TimelineAsset>, String> {
    let payload: TimelineBucketPayload = serde_json::from_slice(&body)
        .map_err(|error| format!("Invalid timeline bucket response: {error}"))?;
    Ok(match payload {
        TimelineBucketPayload::Sequence(assets) => assets
            .into_iter()
            .map(timeline_asset_from_response)
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

fn timeline_asset_from_response(asset: TimelineAssetResponse) -> TimelineAsset {
    TimelineAsset {
        id: asset.id,
        width: if asset.exif.width > 0 {
            asset.exif.width
        } else {
            asset.width.unwrap_or(0)
        },
        height: if asset.exif.height > 0 {
            asset.exif.height
        } else {
            asset.height.unwrap_or(0)
        },
        r#type: asset.asset_type,
        taken_at: asset
            .exif
            .taken_at
            .or(asset.file_created_at)
            .or(asset.local_date_time)
            .or(asset.created_at),
        camera_make: asset.exif.make,
        camera_model: asset.exif.model,
        file_size: asset.exif.file_size,
        duration: asset.duration,
    }
}

fn album_bucket_for(asset: &TimelineAsset) -> Option<String> {
    let captured_at = asset.taken_at.as_deref()?;
    let captured_at = DateTime::parse_from_rfc3339(captured_at).ok()?;
    Some(format!("{}-01T00:00:00.000Z", captured_at.format("%Y-%m")))
}

fn group_album_timeline_assets(assets: Vec<TimelineAsset>) -> AlbumTimeline {
    let mut assets_by_bucket: BTreeMap<String, Vec<TimelineAsset>> = BTreeMap::new();
    for asset in assets {
        // An asset without a trustworthy capture timestamp cannot be placed in
        // the chronological scrubber. Do not misrepresent it as January 1970.
        if let Some(bucket) = album_bucket_for(&asset) {
            assets_by_bucket.entry(bucket).or_default().push(asset);
        }
    }
    for assets in assets_by_bucket.values_mut() {
        assets.sort_by(|left, right| {
            right
                .taken_at
                .cmp(&left.taken_at)
                .then_with(|| right.id.cmp(&left.id))
        });
    }
    let buckets = assets_by_bucket
        .iter()
        .rev()
        .map(|(time_bucket, assets)| TimelineBucket {
            time_bucket: time_bucket.clone(),
            count: assets.len() as u64,
        })
        .collect();
    AlbumTimeline {
        buckets,
        assets_by_bucket,
    }
}

#[cfg(test)]
mod tests {
    use super::{group_album_timeline_assets, ImmichClient, TimelineAsset};
    use crate::immich::AssetDetails;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn groups_album_assets_by_their_local_capture_month_and_skips_dateless_assets() {
        let timeline = group_album_timeline_assets(vec![
            TimelineAsset {
                id: "march-local".to_string(),
                width: 1,
                height: 1,
                r#type: "IMAGE".to_string(),
                taken_at: Some("2025-03-31T23:30:00-02:00".to_string()),
                camera_make: None,
                camera_model: None,
                file_size: None,
                duration: None,
            },
            TimelineAsset {
                id: "dateless".to_string(),
                width: 1,
                height: 1,
                r#type: "IMAGE".to_string(),
                taken_at: None,
                camera_make: None,
                camera_model: None,
                file_size: None,
                duration: None,
            },
        ]);

        assert_eq!(
            timeline.buckets,
            vec![super::TimelineBucket {
                time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                count: 1,
            }]
        );
        assert_eq!(
            timeline.assets_by_bucket["2025-03-01T00:00:00.000Z"][0].id,
            "march-local"
        );
        assert!(!timeline
            .assets_by_bucket
            .contains_key("1970-01-01T00:00:00.000Z"));
    }

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
    async fn fetches_asset_details_with_optional_exif_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /api/assets/asset-1 HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            let body = br#"{"originalFileName":"IMG_0001.CR3","exifInfo":{"fNumber":2.8,"exposureTime":"1/250","iso":100,"focalLength":50,"lensModel":"RF50mm F1.8 STM"}}"#;
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

        let details: AssetDetails = client.get_asset_details("asset-1").await.unwrap();

        assert_eq!(details.original_file_name.as_deref(), Some("IMG_0001.CR3"));
        assert_eq!(details.f_number, Some(2.8));
        assert_eq!(details.exposure_time.as_deref(), Some("1/250"));
        assert_eq!(details.iso, Some(100));
        assert_eq!(details.focal_length, Some(50.0));
        assert_eq!(details.lens_model.as_deref(), Some("RF50mm F1.8 STM"));
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
    async fn fetches_timeline_buckets_and_assets_filtered_to_a_recognized_person() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text
                .starts_with("GET /api/timeline/buckets?size=MONTH&personId=person-1 HTTP/1.1"));
            assert!(request_text.contains("x-api-key: test-key"));
            let buckets = br#"[{"timeBucket":"2025-03-01T00:00:00.000Z","count":1}]"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", buckets.len()).as_bytes()).await.unwrap();
            stream.write_all(buckets).await.unwrap();

            let (mut stream, _) = listener.accept().await.unwrap();
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text.starts_with("GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z&personId=person-1 HTTP/1.1"));
            assert!(request_text.contains("x-api-key: test-key"));
            let assets = br#"[{"id":"asset-1","type":"IMAGE","exifInfo":{"imageWidth":640,"imageHeight":480,"dateTimeOriginal":"2025-03-01T12:30:00.000Z"}}]"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", assets.len()).as_bytes()).await.unwrap();
            stream.write_all(assets).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let buckets = client
            .get_person_timeline_buckets("person-1")
            .await
            .unwrap();
        let assets = client
            .get_person_timeline_bucket_assets("person-1", "2025-03-01T00:00:00.000Z")
            .await
            .unwrap();

        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].count, 1);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, "asset-1");
        assert_eq!(
            assets[0].taken_at.as_deref(),
            Some("2025-03-01T12:30:00.000Z")
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fetches_every_album_metadata_page_before_grouping_a_complete_month_index() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (expected_page, body) in [
                (1, br#"{"assets":{"items":[{"id":"feb","type":"IMAGE","fileCreatedAt":"2025-02-10T10:00:00.000Z","exifInfo":{"imageWidth":640,"imageHeight":480}},{"id":"march-old","type":"VIDEO","duration":"65000","exifInfo":{"dateTimeOriginal":"2025-03-01T10:00:00.000Z"}}],"nextPage":"2"}}"#.as_slice()),
                (2, br#"{"assets":{"items":[{"id":"march-new","type":"IMAGE","width":1200,"height":900,"createdAt":"2025-03-15T10:00:00.000Z","exifInfo":{}}],"nextPage":null}}"#.as_slice()),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let read = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
                assert!(request.contains("x-api-key: test-key"));
                assert!(request.contains(&format!(r#""page":{expected_page}"#)));
                assert!(request.contains(r#""size":1000"#));
                assert!(request.contains(r#""albumIds":["album-1"]"#));
                assert!(request.contains(r#""order":"desc""#));
                assert!(request.contains(r#""withExif":true"#));
                assert!(!request.contains("orderDirection"));
                assert!(!request.contains("/api/albums/album-1"));
                stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
            }
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let timeline = client.get_album_timeline("album-1").await.unwrap();

        assert_eq!(
            timeline.buckets,
            vec![
                super::TimelineBucket {
                    time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                    count: 2
                },
                super::TimelineBucket {
                    time_bucket: "2025-02-01T00:00:00.000Z".to_string(),
                    count: 1
                },
            ]
        );
        let march = timeline
            .assets_by_bucket
            .get("2025-03-01T00:00:00.000Z")
            .unwrap();
        assert_eq!(
            march
                .iter()
                .map(|asset| asset.id.as_str())
                .collect::<Vec<_>>(),
            vec!["march-new", "march-old"]
        );
        assert_eq!(march[0].width, 1200);
        assert_eq!(march[0].height, 900);
        assert_eq!(march[1].duration, Some(65));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_non_advancing_album_metadata_page_markers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            let body = br#"{"assets":{"items":[],"nextPage":1}}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
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

        assert_eq!(
            client.get_album_timeline("album-1").await.unwrap_err(),
            "Invalid album timeline pagination response"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_malformed_album_metadata_responses_without_exposing_the_api_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            let body = br#"{not-json"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "album-api-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let error = client.get_album_timeline("album-1").await.unwrap_err();
        assert!(error.starts_with("Invalid album timeline response:"));
        assert!(!error.contains("album-api-key"));
        server.await.unwrap();
    }
}
