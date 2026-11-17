use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, request_failure_code, response_status_error, send_checked,
    MAX_SUCCESS_RESPONSE_BYTES,
};
use chrono::DateTime;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
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
    pub is_favorite: bool,
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

/// How a library timeline request reports HTTP 403: with the shared Explore permission code
/// (`{prefix}_permission_denied:{hint}`) instead of the generic status message.
#[derive(Clone, Copy)]
struct PermissionCode {
    prefix: &'static str,
    hint: &'static str,
}

/// The Archive Timeline names the missing `asset.read` API key permission.
const ARCHIVE_PERMISSION_CODE: PermissionCode = PermissionCode {
    prefix: "archive",
    hint: "asset.read",
};

/// Like `send_checked`, except that a 403 becomes the credential-free permission code when
/// `permission` is given. Every other failure keeps `send_checked`'s message.
async fn send_timeline_request(
    request: RequestBuilder,
    failure: &str,
    permission: Option<PermissionCode>,
) -> Result<Response, String> {
    let Some(permission) = permission else {
        return send_checked(request, failure).await;
    };
    let response = request
        .send()
        .await
        .map_err(|error| format!("{failure}: {error}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    // The status line is all that leaves this function; the body is discarded unread.
    let detail = response_status_error(response).await?;
    if status == StatusCode::FORBIDDEN {
        return Err(request_failure_code(
            permission.prefix,
            status,
            permission.hint,
        ));
    }
    Err(format!("{failure}: {detail}"))
}

/// A Place Timeline has the same complete, locally indexed shape as an Album Timeline.
pub type PlaceTimeline = AlbumTimeline;

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
pub(super) struct TimelineAssetResponse {
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
    #[serde(rename = "isFavorite", default)]
    is_favorite: Option<bool>,
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
    #[serde(rename = "isFavorite", default)]
    is_favorite: Vec<Option<bool>>,
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

pub(super) fn deserialize_next_page<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
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
        self.search_complete_timeline(serde_json::json!({ "albumIds": [album_id] }), "album")
            .await
    }

    /// Loads a Place Timeline: every Remote Asset Immich records in `city`. Immich's
    /// metadata search matches the city name only, so same-named cities in different
    /// countries share one Place (ADR-0007).
    pub async fn get_place_timeline(&self, city: &str) -> Result<PlaceTimeline, String> {
        self.search_complete_timeline(serde_json::json!({ "city": city }), "place")
            .await
    }

    /// Pages through Immich's metadata search with `filter` until the result is
    /// complete, then groups it by local capture month (ADR-0006). `subject` only
    /// names the timeline kind in error messages.
    async fn search_complete_timeline(
        &self,
        filter: serde_json::Value,
        subject: &str,
    ) -> Result<AlbumTimeline, String> {
        const PAGE_SIZE: u64 = 1_000;
        let mut page = 1_u64;
        let mut assets = Vec::new();

        loop {
            let mut body = filter.clone();
            body["order"] = "desc".into();
            body["withExif"] = true.into();
            body["page"] = page.into();
            body["size"] = PAGE_SIZE.into();
            let response = send_checked(
                self.request(Method::POST, "search/metadata").json(&body),
                &format!("Failed to fetch {subject} timeline"),
            )
            .await?;

            let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
            let payload: MetadataSearchResponse = serde_json::from_slice(&body)
                .map_err(|error| format!("Invalid {subject} timeline response: {error}"))?;
            assets.extend(
                payload
                    .assets
                    .items
                    .into_iter()
                    .map(|asset| timeline_asset_from_response(asset, false)),
            );

            match payload.assets.next_page {
                Some(next_page) if next_page > page => page = next_page,
                Some(_) => return Err(format!("Invalid {subject} timeline pagination response")),
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
            .request(Method::GET, "timeline/buckets")
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
            .request(Method::GET, "timeline/bucket")
            .query(&[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("personId", person_id),
            ])
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
            false,
        )
    }

    pub async fn get_tag_timeline_buckets(
        &self,
        tag_id: &str,
    ) -> Result<Vec<TimelineBucket>, String> {
        Self::validate_path_segment(tag_id, "tag ID")?;
        self.fetch_timeline_buckets_with_query(
            &[("size", "MONTH"), ("tagId", tag_id)],
            "tag timeline buckets",
            None,
        )
        .await
    }

    pub async fn get_tag_timeline_bucket_assets(
        &self,
        tag_id: &str,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        Self::validate_path_segment(tag_id, "tag ID")?;
        self.fetch_timeline_bucket_assets_with_query(
            &[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("tagId", tag_id),
            ],
            "tag timeline bucket",
            false,
            None,
        )
        .await
    }

    async fn fetch_timeline_buckets_with_query(
        &self,
        query: &[(&str, &str)],
        context: &str,
        permission: Option<PermissionCode>,
    ) -> Result<Vec<TimelineBucket>, String> {
        let failure = format!("Failed to fetch {context}");
        let response = send_timeline_request(
            self.request(Method::GET, "timeline/buckets").query(query),
            &failure,
            permission,
        )
        .await?;

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let mut buckets: Vec<TimelineBucket> = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid {context} response: {error}"))?;
        buckets.sort_by(|a, b| b.time_bucket.cmp(&a.time_bucket));
        Ok(buckets)
    }

    async fn fetch_timeline_bucket_assets_with_query(
        &self,
        query: &[(&str, &str)],
        context: &str,
        default_favorite: bool,
        permission: Option<PermissionCode>,
    ) -> Result<Vec<TimelineAsset>, String> {
        let failure = format!("Failed to fetch {context}");
        let response = send_timeline_request(
            self.request(Method::GET, "timeline/bucket").query(query),
            &failure,
            permission,
        )
        .await?;
        parse_timeline_bucket_assets(
            read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
            default_favorite,
        )
    }

    pub async fn get_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        self.fetch_timeline_buckets_with_query(&[("size", "MONTH")], "timeline buckets", None)
            .await
    }

    pub async fn get_timeline_bucket_assets(
        &self,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        self.fetch_timeline_bucket_assets_with_query(
            &[("size", "MONTH"), ("timeBucket", time_bucket)],
            "timeline bucket",
            false,
            None,
        )
        .await
    }

    pub async fn get_favorites_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        self.fetch_timeline_buckets_with_query(
            &[("size", "MONTH"), ("isFavorite", "true")],
            "favorites timeline buckets",
            None,
        )
        .await
    }

    pub async fn get_favorites_timeline_bucket_assets(
        &self,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        self.fetch_timeline_bucket_assets_with_query(
            &[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("isFavorite", "true"),
            ],
            "favorites timeline bucket",
            true,
            None,
        )
        .await
    }

    /// Timeline buckets of the user's Archived Assets (visibility filter `archive`).
    pub async fn get_archive_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        self.fetch_timeline_buckets_with_query(
            &[("size", "MONTH"), ("visibility", "archive")],
            "archive timeline buckets",
            Some(ARCHIVE_PERMISSION_CODE),
        )
        .await
    }

    pub async fn get_archive_timeline_bucket_assets(
        &self,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        self.fetch_timeline_bucket_assets_with_query(
            &[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("visibility", "archive"),
            ],
            "archive timeline bucket",
            false,
            Some(ARCHIVE_PERMISSION_CODE),
        )
        .await
    }

    /// Timeline buckets of the user's Trashed Assets (filter `isTrashed=true`). Observed on
    /// Immich v3.4.1 (one server version only): the parameter is accepted and `asset.read` suffices.
    pub async fn get_trash_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        self.fetch_timeline_buckets_with_query(
            &[("size", "MONTH"), ("isTrashed", "true")],
            "trash timeline buckets",
            None,
        )
        .await
    }

    pub async fn get_trash_timeline_bucket_assets(
        &self,
        time_bucket: &str,
    ) -> Result<Vec<TimelineAsset>, String> {
        self.fetch_timeline_bucket_assets_with_query(
            &[
                ("size", "MONTH"),
                ("timeBucket", time_bucket),
                ("isTrashed", "true"),
            ],
            "trash timeline bucket",
            false,
            None,
        )
        .await
    }
}

fn parse_timeline_bucket_assets(
    body: Vec<u8>,
    default_favorite: bool,
) -> Result<Vec<TimelineAsset>, String> {
    let payload: TimelineBucketPayload = serde_json::from_slice(&body)
        .map_err(|error| format!("Invalid timeline bucket response: {error}"))?;
    Ok(match payload {
        TimelineBucketPayload::Sequence(assets) => assets
            .into_iter()
            .map(|asset| timeline_asset_from_response(asset, default_favorite))
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
                    is_favorite: columnar
                        .is_favorite
                        .get(index)
                        .and_then(|value| *value)
                        .unwrap_or(default_favorite),
                }
            })
            .collect(),
    })
}

pub(super) fn timeline_asset_from_response(
    asset: TimelineAssetResponse,
    default_favorite: bool,
) -> TimelineAsset {
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
        is_favorite: asset.is_favorite.unwrap_or(default_favorite),
    }
}

fn album_bucket_for(asset: &TimelineAsset) -> Option<String> {
    let captured_at = asset.taken_at.as_deref()?;
    let captured_at = DateTime::parse_from_rfc3339(captured_at).ok()?;
    Some(format!("{}-01T00:00:00.000Z", captured_at.format("%Y-%m")))
}

pub(super) fn group_album_timeline_assets(assets: Vec<TimelineAsset>) -> AlbumTimeline {
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
    use crate::immich::test_support::{serve_pages, test_client};
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
                is_favorite: false,
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
                is_favorite: false,
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
            let body = br#"{"originalFileName":"IMG_0001.CR3","isFavorite":true,"fileCreatedAt":"2024-01-01T00:00:00Z","exifInfo":{"dateTimeOriginal":"2023-12-25T12:30:00Z","make":"Canon","model":"EOS R6","city":"Berlin","state":"Berlin","country":"Germany","fNumber":2.8,"exposureTime":"1/250","iso":100,"focalLength":50,"lensModel":"RF50mm F1.8 STM","exifImageWidth":6048,"exifImageHeight":4024,"fileSizeInByte":12345678}}"#;
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
        assert!(details.is_favorite);
        assert_eq!(details.taken_at.as_deref(), Some("2023-12-25T12:30:00Z"));
        assert_eq!(details.camera_make.as_deref(), Some("Canon"));
        assert_eq!(details.camera_model.as_deref(), Some("EOS R6"));
        assert_eq!(details.city.as_deref(), Some("Berlin"));
        assert_eq!(details.state.as_deref(), Some("Berlin"));
        assert_eq!(details.country.as_deref(), Some("Germany"));
        assert_eq!(details.f_number, Some(2.8));
        assert_eq!(details.exposure_time.as_deref(), Some("1/250"));
        assert_eq!(details.iso, Some(100));
        assert_eq!(details.focal_length, Some(50.0));
        assert_eq!(details.lens_model.as_deref(), Some("RF50mm F1.8 STM"));
        assert_eq!(details.image_width, Some(6048));
        assert_eq!(details.image_height, Some(4024));
        assert_eq!(details.file_size, Some(12345678));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn omits_missing_and_blank_asset_details() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for body in [
                br#"{"originalFileName":" "}"#.as_slice(),
                br#"{"originalFileName":" ","exifInfo":{"exposureTime":" ","lensModel":""}}"#
                    .as_slice(),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                assert!(request.starts_with("GET /api/assets/asset-1 HTTP/1.1"));
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

        for _ in 0..2 {
            assert_eq!(
                client.get_asset_details("asset-1").await.unwrap(),
                AssetDetails {
                    asset_type: None,
                    taken_at: None,
                    original_file_name: None,
                    is_favorite: false,
                    camera_make: None,
                    camera_model: None,
                    city: None,
                    state: None,
                    country: None,
                    f_number: None,
                    exposure_time: None,
                    iso: None,
                    focal_length: None,
                    lens_model: None,
                    image_width: None,
                    image_height: None,
                    file_size: None,
                }
            );
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reports_asset_detail_response_errors() {
        let (url, server) = serve_pages(vec![("500 Internal Server Error", "server error")]).await;

        let error = test_client(url, "test-key")
            .get_asset_details("asset-1")
            .await
            .unwrap_err();

        assert!(error.starts_with("Failed to fetch asset details:"));
        assert!(server.await.unwrap()[0].starts_with("GET /api/assets/asset-1 HTTP/1.1"));
    }

    #[tokio::test]
    async fn updates_an_asset_favorite_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("PUT /api/assets/asset-1 HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            assert!(request.contains("{\"isFavorite\":true}"));
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        client.set_asset_favorite("asset-1", true).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn reports_asset_favorite_update_response_errors() {
        let (url, server) = serve_pages(vec![("500 Internal Server Error", "server error")]).await;

        let error = test_client(url, "test-key")
            .set_asset_favorite("asset-1", true)
            .await
            .unwrap_err();

        assert!(error.starts_with("Failed to update asset favorite:"));
        assert!(server.await.unwrap()[0].starts_with("PUT /api/assets/asset-1 HTTP/1.1"));
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
            let assets_body = br#"{"id":["photo-1","video-1","video-2","video-3"],"isImage":[true,false,false,false],"isFavorite":[false,true,false,false],"ratio":[0.67,1.7778,1.7778,1.7778],"duration":[null,"0:01:05.000",2205,"5231"],"fileCreatedAt":["2026-11-04T19:47:41.29","2026-11-03T10:00:00.000Z","2026-11-02T10:00:00.000Z","2026-11-01T10:00:00.000Z"]}"#;
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
        assert!(!assets[0].is_favorite);

        assert_eq!(assets[1].id, "video-1");
        assert_eq!(assets[1].width, 1778);
        assert_eq!(assets[1].height, 1000);
        assert!(assets[1].is_favorite);
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
    async fn fetches_timeline_buckets_and_assets_filtered_to_a_tag() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text
                .starts_with("GET /api/timeline/buckets?size=MONTH&tagId=tag-1 HTTP/1.1"));
            assert!(request_text.contains("x-api-key: test-key"));
            let buckets = br#"[{"timeBucket":"2025-02-01T00:00:00.000Z","count":1},{"timeBucket":"2025-03-01T00:00:00.000Z","count":1}]"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", buckets.len()).as_bytes()).await.unwrap();
            stream.write_all(buckets).await.unwrap();

            let (mut stream, _) = listener.accept().await.unwrap();
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text.starts_with("GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z&tagId=tag-1 HTTP/1.1"));
            assert!(request_text.contains("x-api-key: test-key"));
            assert!(!request_text.contains("tagIds"));
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

        let buckets = client.get_tag_timeline_buckets("tag-1").await.unwrap();
        let assets = client
            .get_tag_timeline_bucket_assets("tag-1", "2025-03-01T00:00:00.000Z")
            .await
            .unwrap();

        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].time_bucket, "2025-03-01T00:00:00.000Z");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, "asset-1");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_tag_timeline_request_never_names_a_permission_or_leaks_the_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for status in ["403 Forbidden", "500 Internal Server Error"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
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

        let forbidden = client.get_tag_timeline_buckets("tag-1").await.unwrap_err();
        let failed = client
            .get_tag_timeline_bucket_assets("tag-1", "2025-03-01T00:00:00.000Z")
            .await
            .unwrap_err();

        for error in [forbidden, failed] {
            assert!(!error.contains("permission_denied"), "{error}");
            assert!(!error.contains("test-key"), "{error}");
        }
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
        let (url, server) =
            serve_pages(vec![("200 OK", r#"{"assets":{"items":[],"nextPage":1}}"#)]).await;

        assert_eq!(
            test_client(url, "test-key")
                .get_album_timeline("album-1")
                .await
                .unwrap_err(),
            "Invalid album timeline pagination response"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_malformed_album_metadata_responses_without_exposing_the_api_key() {
        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;

        let error = test_client(url, "album-api-key")
            .get_album_timeline("album-1")
            .await
            .unwrap_err();
        assert!(error.starts_with("Invalid album timeline response:"));
        assert!(!error.contains("album-api-key"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fetches_every_city_filtered_metadata_page_and_groups_dated_assets_by_month() {
        let (url, server) = serve_pages(vec![
            ("200 OK", r#"{"assets":{"items":[{"id":"feb","type":"IMAGE","fileCreatedAt":"2025-02-10T10:00:00.000Z","exifInfo":{}},{"id":"undated","type":"IMAGE","exifInfo":{}}],"nextPage":"2"}}"#),
            ("200 OK", r#"{"assets":{"items":[{"id":"march","type":"IMAGE","fileCreatedAt":"2025-03-15T10:00:00.000Z","exifInfo":{}}],"nextPage":null}}"#),
        ])
        .await;

        let timeline = test_client(url, "test-key")
            .get_place_timeline("Paris")
            .await
            .unwrap();

        let requests = server.await.unwrap();
        for (index, request) in requests.iter().enumerate() {
            assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            assert!(request.contains(r#""city":"Paris""#));
            assert!(!request.contains("albumIds"));
            assert!(request.contains(r#""size":1000"#));
            assert!(request.contains(r#""order":"desc""#));
            assert!(request.contains(r#""withExif":true"#));
            assert!(request.contains(&format!(r#""page":{}"#, index + 1)));
        }
        assert_eq!(
            timeline.buckets,
            vec![
                super::TimelineBucket {
                    time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                    count: 1
                },
                super::TimelineBucket {
                    time_bucket: "2025-02-01T00:00:00.000Z".to_string(),
                    count: 1
                },
            ]
        );
        assert!(timeline
            .assets_by_bucket
            .values()
            .flatten()
            .all(|asset| asset.id != "undated"));
    }

    #[tokio::test]
    async fn rejects_non_advancing_place_metadata_page_markers() {
        let (url, server) =
            serve_pages(vec![("200 OK", r#"{"assets":{"items":[],"nextPage":1}}"#)]).await;

        assert_eq!(
            test_client(url, "test-key")
                .get_place_timeline("Paris")
                .await
                .unwrap_err(),
            "Invalid place timeline pagination response"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fails_the_whole_place_load_when_a_later_page_fails() {
        let (url, server) = serve_pages(vec![
            (
                "200 OK",
                r#"{"assets":{"items":[{"id":"a","type":"IMAGE","fileCreatedAt":"2025-02-10T10:00:00.000Z","exifInfo":{}}],"nextPage":"2"}}"#,
            ),
            ("500 Internal Server Error", "secret-detail"),
        ])
        .await;

        let error = test_client(url, "place-api-key")
            .get_place_timeline("Paris")
            .await
            .unwrap_err();

        assert!(error.starts_with("Failed to fetch place timeline:"));
        assert!(!error.contains("secret-detail"));
        assert!(!error.contains("place-api-key"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn maps_place_timeline_status_and_malformed_body_errors_without_leaking_either() {
        let (url, server) = serve_pages(vec![("403 Forbidden", "secret-detail")]).await;
        let error = test_client(url, "place-api-key")
            .get_place_timeline("Paris")
            .await
            .unwrap_err();
        assert!(error.starts_with("Failed to fetch place timeline:"));
        assert!(error.contains("403"));
        assert!(!error.contains("secret-detail"));
        server.await.unwrap();

        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;
        let error = test_client(url, "place-api-key")
            .get_place_timeline("Paris")
            .await
            .unwrap_err();
        assert!(error.starts_with("Invalid place timeline response:"));
        assert!(!error.contains("place-api-key"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fetches_favorites_timeline_buckets_and_assets_with_favorite_default() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text
                .starts_with("GET /api/timeline/buckets?size=MONTH&isFavorite=true HTTP/1.1"));
            assert!(request_text.contains("x-api-key: test-key"));
            assert!(!request_text.contains("withPartners"));
            let buckets = br#"[{"timeBucket":"2025-02-01T00:00:00.000Z","count":1},{"timeBucket":"2025-03-01T00:00:00.000Z","count":2}]"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        buckets.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(buckets).await.unwrap();

            let (mut stream, _) = listener.accept().await.unwrap();
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text.starts_with(
                "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z&isFavorite=true HTTP/1.1"
            ));
            assert!(request_text.contains("x-api-key: test-key"));
            let assets = br#"{"id":["fav-photo","fav-video"],"isImage":[true,false],"isFavorite":[null],"ratio":[1.5,1.7778],"duration":[null,"0:00:42.000"],"fileCreatedAt":["2025-03-10T10:00:00.000Z","2025-03-05T10:00:00.000Z"]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        assets.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(assets).await.unwrap();

            let (mut stream, _) = listener.accept().await.unwrap();
            let read = stream.read(&mut request).await.unwrap();
            let request_text = String::from_utf8_lossy(&request[..read]);
            assert!(request_text.starts_with(
                "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-02-01T00%3A00%3A00.000Z&isFavorite=true HTTP/1.1"
            ));
            let legacy_assets = br#"[{"id":"legacy-fav","type":"IMAGE","fileCreatedAt":"2025-02-14T10:00:00.000Z"}]"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        legacy_assets.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(legacy_assets).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let buckets = client.get_favorites_timeline_buckets().await.unwrap();
        assert_eq!(
            buckets,
            vec![
                super::TimelineBucket {
                    time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                    count: 2,
                },
                super::TimelineBucket {
                    time_bucket: "2025-02-01T00:00:00.000Z".to_string(),
                    count: 1,
                },
            ]
        );

        let march_assets = client
            .get_favorites_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(march_assets.len(), 2);
        assert_eq!(march_assets[0].id, "fav-photo");
        assert_eq!(march_assets[0].r#type, "IMAGE");
        assert!(march_assets[0].is_favorite);
        assert_eq!(march_assets[1].id, "fav-video");
        assert_eq!(march_assets[1].r#type, "VIDEO");
        assert_eq!(march_assets[1].duration, Some(42));
        assert!(march_assets[1].is_favorite);

        let feb_assets = client
            .get_favorites_timeline_bucket_assets("2025-02-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(feb_assets.len(), 1);
        assert_eq!(feb_assets[0].id, "legacy-fav");
        assert!(feb_assets[0].is_favorite);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn reports_favorites_timeline_errors_without_leaking_body_or_api_key() {
        let (url, server) = serve_pages(vec![(
            "500 Internal Server Error",
            "internal secret stacktrace",
        )])
        .await;

        let error = test_client(url, "fav-secret-key")
            .get_favorites_timeline_buckets()
            .await
            .unwrap_err();
        assert!(error.starts_with("Failed to fetch favorites timeline buckets:"));
        assert!(!error.contains("internal secret stacktrace"));
        assert!(!error.contains("fav-secret-key"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fetches_archive_timeline_buckets_and_assets_with_archive_visibility() {
        let (url, server) = serve_pages(vec![
            (
                "200 OK",
                r#"[{"timeBucket":"2025-02-01T00:00:00.000Z","count":1},{"timeBucket":"2025-03-01T00:00:00.000Z","count":2}]"#,
            ),
            (
                "200 OK",
                r#"{"id":["arch-photo","arch-video"],"isImage":[true,false],"isFavorite":[null,true],"ratio":[1.5,1.7778],"duration":[null,"0:00:42.000"],"fileCreatedAt":["2025-03-10T10:00:00.000Z","2025-03-05T10:00:00.000Z"]}"#,
            ),
            (
                "200 OK",
                r#"[{"id":"legacy-arch","type":"IMAGE","fileCreatedAt":"2025-02-14T10:00:00.000Z"}]"#,
            ),
        ])
        .await;
        let client = test_client(url, "test-key");

        let buckets = client.get_archive_timeline_buckets().await.unwrap();
        assert_eq!(
            buckets,
            vec![
                super::TimelineBucket {
                    time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                    count: 2,
                },
                super::TimelineBucket {
                    time_bucket: "2025-02-01T00:00:00.000Z".to_string(),
                    count: 1,
                },
            ]
        );
        let march = client
            .get_archive_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(march.len(), 2);
        assert_eq!(march[0].id, "arch-photo");
        assert_eq!(march[0].r#type, "IMAGE");
        assert!(!march[0].is_favorite);
        assert_eq!(march[1].id, "arch-video");
        assert_eq!(march[1].r#type, "VIDEO");
        assert_eq!(march[1].duration, Some(42));
        assert!(march[1].is_favorite);
        let february = client
            .get_archive_timeline_bucket_assets("2025-02-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(february.len(), 1);
        assert_eq!(february[0].id, "legacy-arch");
        assert!(!february[0].is_favorite);

        let requests = server.await.unwrap();
        assert!(requests[0]
            .starts_with("GET /api/timeline/buckets?size=MONTH&visibility=archive HTTP/1.1"));
        assert!(requests[1].starts_with(
            "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z&visibility=archive HTTP/1.1"
        ));
        assert!(requests[2].starts_with(
            "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-02-01T00%3A00%3A00.000Z&visibility=archive HTTP/1.1"
        ));
        for request in &requests {
            assert!(request.contains("x-api-key: test-key"));
            assert!(!request.contains("isFavorite"));
            assert!(!request.contains("isArchived"));
            assert!(!request.contains("isTrashed"));
            assert!(!request.contains("withPartners"));
        }
    }

    #[tokio::test]
    async fn classifies_archive_timeline_failures_without_leaking_body_or_api_key() {
        for status in ["401 Unauthorized", "500 Internal Server Error"] {
            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "arch-secret-key")
                .get_archive_timeline_buckets()
                .await
                .unwrap_err();
            server.await.unwrap();
            assert!(error.starts_with("Failed to fetch archive timeline buckets:"));
            assert!(error.contains(&status[..3]), "{error}");
            assert!(!error.contains("internal secret stacktrace"));
            assert!(!error.contains("arch-secret-key"));

            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "arch-secret-key")
                .get_archive_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
                .await
                .unwrap_err();
            server.await.unwrap();
            assert!(error.starts_with("Failed to fetch archive timeline bucket:"));
            assert!(error.contains(&status[..3]), "{error}");
            assert!(!error.contains("internal secret stacktrace"));
            assert!(!error.contains("arch-secret-key"));
        }

        // A permission failure carries a stable, credential-free code that names the missing permission.
        let (url, server) =
            serve_pages(vec![("403 Forbidden", "internal secret stacktrace")]).await;
        let error = test_client(url, "arch-secret-key")
            .get_archive_timeline_buckets()
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "archive_permission_denied:asset.read");

        let (url, server) =
            serve_pages(vec![("403 Forbidden", "internal secret stacktrace")]).await;
        let error = test_client(url, "arch-secret-key")
            .get_archive_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "archive_permission_denied:asset.read");

        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;
        let error = test_client(url, "arch-secret-key")
            .get_archive_timeline_buckets()
            .await
            .unwrap_err();
        server.await.unwrap();
        assert!(error.starts_with("Invalid archive timeline buckets response:"));
        assert!(!error.contains("arch-secret-key"));

        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;
        let error = test_client(url, "arch-secret-key")
            .get_archive_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap_err();
        server.await.unwrap();
        assert!(error.starts_with("Invalid timeline bucket response:"));
        assert!(!error.contains("arch-secret-key"));
    }

    #[tokio::test]
    async fn fetches_trash_timeline_buckets_and_assets_with_the_trashed_filter() {
        let (url, server) = serve_pages(vec![
            (
                "200 OK",
                r#"[{"timeBucket":"2025-02-01T00:00:00.000Z","count":1},{"timeBucket":"2025-03-01T00:00:00.000Z","count":2}]"#,
            ),
            (
                "200 OK",
                r#"{"id":["trash-photo","trash-video"],"isImage":[true,false],"isFavorite":[null,true],"ratio":[1.5,1.7778],"duration":[null,"0:00:42.000"],"fileCreatedAt":["2025-03-10T10:00:00.000Z","2025-03-05T10:00:00.000Z"]}"#,
            ),
            (
                "200 OK",
                r#"[{"id":"legacy-trash","type":"IMAGE","fileCreatedAt":"2025-02-14T10:00:00.000Z"}]"#,
            ),
        ])
        .await;
        let client = test_client(url, "test-key");

        let buckets = client.get_trash_timeline_buckets().await.unwrap();
        assert_eq!(
            buckets,
            vec![
                super::TimelineBucket {
                    time_bucket: "2025-03-01T00:00:00.000Z".to_string(),
                    count: 2,
                },
                super::TimelineBucket {
                    time_bucket: "2025-02-01T00:00:00.000Z".to_string(),
                    count: 1,
                },
            ]
        );
        let march = client
            .get_trash_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(march.len(), 2);
        assert_eq!(march[0].id, "trash-photo");
        assert_eq!(march[0].r#type, "IMAGE");
        assert!(!march[0].is_favorite);
        assert_eq!(march[1].id, "trash-video");
        assert_eq!(march[1].r#type, "VIDEO");
        assert_eq!(march[1].duration, Some(42));
        assert!(march[1].is_favorite);
        let february = client
            .get_trash_timeline_bucket_assets("2025-02-01T00:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(february.len(), 1);
        assert_eq!(february[0].id, "legacy-trash");
        assert!(!february[0].is_favorite);

        let requests = server.await.unwrap();
        assert!(
            requests[0].starts_with("GET /api/timeline/buckets?size=MONTH&isTrashed=true HTTP/1.1")
        );
        assert!(requests[1].starts_with(
            "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-03-01T00%3A00%3A00.000Z&isTrashed=true HTTP/1.1"
        ));
        assert!(requests[2].starts_with(
            "GET /api/timeline/bucket?size=MONTH&timeBucket=2025-02-01T00%3A00%3A00.000Z&isTrashed=true HTTP/1.1"
        ));
        for request in &requests {
            assert!(request.contains("x-api-key: test-key"));
            assert!(!request.contains("isFavorite"));
            assert!(!request.contains("visibility"));
            assert!(!request.contains("isArchived"));
            assert!(!request.contains("withPartners"));
        }
    }

    #[tokio::test]
    async fn classifies_trash_timeline_failures_without_leaking_body_or_api_key() {
        for status in [
            "401 Unauthorized",
            "403 Forbidden",
            "500 Internal Server Error",
        ] {
            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "trash-secret-key")
                .get_trash_timeline_buckets()
                .await
                .unwrap_err();
            server.await.unwrap();
            assert!(error.starts_with("Failed to fetch trash timeline buckets:"));
            assert!(error.contains(&status[..3]), "{error}");
            assert!(!error.contains("internal secret stacktrace"));
            assert!(!error.contains("trash-secret-key"));

            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "trash-secret-key")
                .get_trash_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
                .await
                .unwrap_err();
            server.await.unwrap();
            assert!(error.starts_with("Failed to fetch trash timeline bucket:"));
            assert!(error.contains(&status[..3]), "{error}");
            assert!(!error.contains("internal secret stacktrace"));
            assert!(!error.contains("trash-secret-key"));
        }

        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;
        let error = test_client(url, "trash-secret-key")
            .get_trash_timeline_buckets()
            .await
            .unwrap_err();
        server.await.unwrap();
        assert!(error.starts_with("Invalid trash timeline buckets response:"));
        assert!(!error.contains("trash-secret-key"));

        let (url, server) = serve_pages(vec![("200 OK", "{not-json")]).await;
        let error = test_client(url, "trash-secret-key")
            .get_trash_timeline_bucket_assets("2025-03-01T00:00:00.000Z")
            .await
            .unwrap_err();
        server.await.unwrap();
        assert!(error.starts_with("Invalid timeline bucket response:"));
        assert!(!error.contains("trash-secret-key"));
    }
}
