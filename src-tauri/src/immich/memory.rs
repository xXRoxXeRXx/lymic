use super::client::ImmichClient;
use super::response::read_response_body_limited;
use super::timeline::{
    group_album_timeline_assets, timeline_asset_from_response, AlbumTimeline, TimelineAsset,
    TimelineAssetResponse,
};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

// Memories embed their full asset lists with EXIF, so they exceed the default cap.
const MAX_MEMORIES_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryKind {
    OnThisDay,
    Birthday,
    Unknown,
}

/// A Memory with the Remote Assets Immich selected for it. Immich returns the
/// assets inside the memory itself, so the Memory Timeline needs no further
/// search; they are grouped into Timeline Buckets here like an Album Timeline.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub kind: MemoryKind,
    pub memory_at: Option<String>,
    pub year: Option<i32>,
    pub person_name: Option<String>,
    pub cover_asset_id: String,
    pub timeline: AlbumTimeline,
}

#[derive(Deserialize)]
struct MemoryResponse {
    id: String,
    #[serde(rename = "type", default)]
    memory_type: Option<String>,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(rename = "memoryAt", default)]
    memory_at: Option<String>,
    #[serde(default)]
    assets: Option<Vec<TimelineAssetResponse>>,
}

fn memory_from_response(response: MemoryResponse) -> Option<Memory> {
    let assets: Vec<TimelineAsset> = response
        .assets
        .unwrap_or_default()
        .into_iter()
        .map(|asset| timeline_asset_from_response(asset, false))
        .collect();
    // A Memory without assets has nothing to show or open.
    let cover_asset_id = assets.first()?.id.clone();
    let kind = match response.memory_type.as_deref() {
        Some("on_this_day") => MemoryKind::OnThisDay,
        Some("birthday") => MemoryKind::Birthday,
        _ => MemoryKind::Unknown,
    };
    let data = response.data.as_ref();
    let year = data
        .and_then(|data| data.get("year"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|year| i32::try_from(year).ok());
    let person_name = data
        .and_then(|data| data.get("personName").or_else(|| data.get("name")))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    Some(Memory {
        id: response.id,
        kind,
        memory_at: response.memory_at,
        year,
        person_name,
        cover_asset_id,
        timeline: group_album_timeline_assets(assets),
    })
}

fn memories_access_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "memories_authentication_required".to_string(),
        StatusCode::FORBIDDEN => "memories_permission_denied:configure an API key with memory.read and asset.read permissions".to_string(),
        _ => "memories_unavailable".to_string(),
    }
}

impl ImmichClient {
    /// The newest Memories (`GET /api/memories`, permission `memory.read`) with
    /// their assets. Immich memory preferences are deliberately not evaluated.
    pub async fn get_memories(&self, size: u32) -> Result<Vec<Memory>, String> {
        let response = self
            .request(Method::GET, "memories")
            .query(&[("size", size.to_string().as_str()), ("order", "desc")])
            .send()
            .await
            .map_err(|_| "memories_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(memories_access_error(response.status()));
        }
        let body = read_response_body_limited(response, MAX_MEMORIES_RESPONSE_BYTES).await?;
        let memories: Vec<MemoryResponse> =
            serde_json::from_slice(&body).map_err(|_| "memories_invalid_response".to_string())?;
        Ok(memories
            .into_iter()
            .filter_map(memory_from_response)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::immich::ImmichClient;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves one canned response and returns the request line and headers received.
    async fn get_memories_from(
        status: &str,
        body: &str,
    ) -> (Result<Vec<super::Memory>, String>, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "memory-api-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let result = client.get_memories(12).await;
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn requests_the_newest_memories_and_groups_their_assets_into_a_timeline() {
        let body = r#"[{"id":"m-1","type":"on_this_day","data":{"year":2020},"memoryAt":"2020-11-15T00:00:00.000Z","assets":[
            {"id":"a-1","type":"IMAGE","fileCreatedAt":"2020-11-15T10:00:00.000Z","isFavorite":true,"exifInfo":{"exifImageWidth":400,"exifImageHeight":300}},
            {"id":"a-2","type":"VIDEO","duration":"0:00:05.000000","fileCreatedAt":"2020-11-15T09:00:00.000Z","exifInfo":{}}]}]"#;

        let (result, request) = get_memories_from("200 OK", body).await;

        assert!(request.starts_with("GET /api/memories?size=12&order=desc HTTP/1.1"));
        assert!(request.contains("x-api-key: memory-api-key"));
        let memories = result.unwrap();
        assert_eq!(memories.len(), 1);
        let memory = &memories[0];
        assert_eq!(memory.id, "m-1");
        assert_eq!(memory.kind, super::MemoryKind::OnThisDay);
        assert_eq!(memory.year, Some(2020));
        assert_eq!(
            memory.memory_at.as_deref(),
            Some("2020-11-15T00:00:00.000Z")
        );
        assert_eq!(memory.cover_asset_id, "a-1");
        assert_eq!(memory.timeline.buckets.len(), 1);
        assert_eq!(
            memory.timeline.buckets[0].time_bucket,
            "2020-11-01T00:00:00.000Z"
        );
        let assets = &memory.timeline.assets_by_bucket["2020-11-01T00:00:00.000Z"];
        assert_eq!(
            assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["a-1", "a-2"]
        );
        assert!(assets[0].is_favorite);
        assert_eq!((assets[0].width, assets[0].height), (400, 300));
    }

    #[tokio::test]
    async fn parses_birthday_and_unknown_types_and_tolerates_missing_optional_fields() {
        let body = r#"[
            {"id":"b","type":"birthday","data":{"personName":"Ada","year":2015},"memoryAt":"2015-05-01T00:00:00.000Z","assets":[{"id":"b-1","type":"IMAGE","fileCreatedAt":"2015-05-01T10:00:00.000Z"}]},
            {"id":"u","type":"holiday_special","assets":[{"id":"u-1","type":"IMAGE"}]},
            {"id":"empty","type":"on_this_day","data":{"year":2019},"assets":[]},
            {"id":"noassets","type":"on_this_day"}]"#;

        let (result, _) = get_memories_from("200 OK", body).await;

        let memories = result.unwrap();
        assert_eq!(
            memories.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "u"]
        );
        assert_eq!(memories[0].kind, super::MemoryKind::Birthday);
        assert_eq!(memories[0].person_name.as_deref(), Some("Ada"));
        assert_eq!(memories[1].kind, super::MemoryKind::Unknown);
        assert_eq!(memories[1].memory_at, None);
        assert_eq!(memories[1].year, None);
        assert_eq!(memories[1].person_name, None);
        assert_eq!(memories[1].cover_asset_id, "u-1");
        // A cover can exist even when no asset has a date to place it in a bucket.
        assert!(memories[1].timeline.buckets.is_empty());
    }

    #[tokio::test]
    async fn maps_access_failures_without_exposing_the_api_key_or_response_body() {
        let (unauthorized, _) =
            get_memories_from("401 Unauthorized", "memory-api-key leaked").await;
        assert_eq!(
            unauthorized.unwrap_err(),
            "memories_authentication_required"
        );

        let (forbidden, _) = get_memories_from("403 Forbidden", "{}").await;
        assert_eq!(
            forbidden.unwrap_err(),
            "memories_permission_denied:configure an API key with memory.read and asset.read permissions"
        );

        let (failed, _) =
            get_memories_from("500 Internal Server Error", "boom memory-api-key").await;
        assert_eq!(failed.unwrap_err(), "memories_unavailable");

        let (malformed, _) = get_memories_from("200 OK", "{\"not\":\"an array\"}").await;
        assert_eq!(malformed.unwrap_err(), "memories_invalid_response");
    }
}
