use super::client::ImmichClient;
use super::response::{read_response_body_limited, request_failure_code};
use super::timeline::{
    group_album_timeline_assets, timeline_asset_from_response, AlbumTimeline, TimelineAsset,
    TimelineAssetResponse,
};
use reqwest::Method;
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
    let timeline = group_album_timeline_assets(assets.clone());
    // The cover must be an asset the Memory Timeline shows, so assets without a
    // capture date (which the timeline omits) never qualify. A Memory with none
    // has nothing to show or open.
    let cover_asset_id = assets
        .iter()
        .find(|asset| {
            timeline
                .assets_by_bucket
                .values()
                .flatten()
                .any(|placed| placed.id == asset.id)
        })?
        .id
        .clone();
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
        timeline,
    })
}

/// One page of Memories for the All Memories view.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoriesPage {
    pub memories: Vec<Memory>,
    pub has_next_page: bool,
}

impl ImmichClient {
    async fn fetch_memory_responses(&self, size: u32) -> Result<Vec<MemoryResponse>, String> {
        let response = self
            .request(Method::GET, "memories")
            .query(&[("size", size.to_string().as_str()), ("order", "desc")])
            .send()
            .await
            .map_err(|_| "memories_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(request_failure_code(
                "memories",
                response.status(),
                "configure an API key with the memory.read permission",
            ));
        }
        let body = read_response_body_limited(response, MAX_MEMORIES_RESPONSE_BYTES).await?;
        serde_json::from_slice(&body).map_err(|_| "memories_invalid_response".to_string())
    }

    /// The newest Memories (`GET /api/memories`, permission `memory.read`) with
    /// their assets. Immich memory preferences are deliberately not evaluated.
    pub async fn get_memories(&self, size: u32) -> Result<Vec<Memory>, String> {
        Ok(self
            .fetch_memory_responses(size)
            .await?
            .into_iter()
            .filter_map(memory_from_response)
            .collect())
    }

    /// One page (1-based) of the newest Memories. Immich's `GET /api/memories`
    /// has no offset or page parameter, so this asks for everything up to one
    /// past the page and drops the earlier pages; the extra Memory tells whether
    /// a next page exists. Paging counts Memories as Immich returns them, before
    /// Memories without dated assets are dropped, so pages stay aligned.
    pub async fn get_memories_page(&self, page: u32, size: u32) -> Result<MemoriesPage, String> {
        let page = page.max(1);
        let size = size.max(1);
        let skip = (page - 1).saturating_mul(size);
        let window_end = skip.saturating_add(size);
        let mut responses = self
            .fetch_memory_responses(window_end.saturating_add(1))
            .await?;
        let has_next_page = responses.len() > window_end as usize;
        responses.truncate(window_end as usize);
        let memories = responses
            .into_iter()
            .skip(skip as usize)
            .filter_map(memory_from_response)
            .collect();
        Ok(MemoriesPage {
            memories,
            has_next_page,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::immich::test_support::{serve_once, test_client};

    /// Serves one canned response and returns the request line and headers received.
    async fn get_memories_from(
        status: &str,
        body: &str,
    ) -> (Result<Vec<super::Memory>, String>, String) {
        let (url, server) = serve_once(status, body).await;
        let result = test_client(url, "memory-api-key").get_memories(12).await;
        (result, server.await.unwrap())
    }

    /// Serves one canned response to a paged request and returns the request line and headers received.
    async fn get_memories_page_from(
        page: u32,
        size: u32,
        status: &str,
        body: &str,
    ) -> (Result<super::MemoriesPage, String>, String) {
        let (url, server) = serve_once(status, body).await;
        let result = test_client(url, "memory-api-key")
            .get_memories_page(page, size)
            .await;
        (result, server.await.unwrap())
    }

    fn memory_json(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","type":"on_this_day","data":{{"year":2020}},"assets":[{{"id":"{id}-cover","type":"IMAGE","fileCreatedAt":"2020-11-15T10:00:00.000Z"}}]}}"#
        )
    }

    fn memories_json(ids: &[&str]) -> String {
        format!(
            "[{}]",
            ids.iter()
                .map(|id| memory_json(id))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    #[tokio::test]
    async fn pages_through_memories_by_requesting_one_more_than_the_window_and_skipping_earlier_pages(
    ) {
        // Immich has no offset: page 2 of size 2 asks for the newest 5 and shows items 3 and 4.
        let body = memories_json(&["m1", "m2", "m3", "m4", "m5"]);

        let (result, request) = get_memories_page_from(2, 2, "200 OK", &body).await;

        assert!(request.starts_with("GET /api/memories?size=5&order=desc HTTP/1.1"));
        let page = result.unwrap();
        assert_eq!(
            page.memories
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["m3", "m4"]
        );
        assert!(page.has_next_page);
    }

    #[tokio::test]
    async fn reports_no_next_page_when_immich_returns_no_more_than_the_window() {
        let (last, _) =
            get_memories_page_from(2, 2, "200 OK", &memories_json(&["m1", "m2", "m3", "m4"])).await;
        let last = last.unwrap();
        assert_eq!(last.memories.len(), 2);
        assert!(!last.has_next_page);

        let (past_end, _) =
            get_memories_page_from(3, 2, "200 OK", &memories_json(&["m1", "m2", "m3"])).await;
        let past_end = past_end.unwrap();
        assert!(past_end.memories.is_empty());
        assert!(!past_end.has_next_page);
    }

    #[tokio::test]
    async fn paging_counts_memories_without_assets_so_pages_stay_aligned() {
        let body = r#"[
            {"id":"empty","type":"on_this_day","assets":[]},
            {"id":"m2","type":"on_this_day","assets":[{"id":"c2","type":"IMAGE","fileCreatedAt":"2020-01-01T10:00:00.000Z"}]},
            {"id":"m3","type":"on_this_day","assets":[{"id":"c3","type":"IMAGE","fileCreatedAt":"2020-01-01T10:00:00.000Z"}]}]"#;

        let (result, _) = get_memories_page_from(1, 2, "200 OK", body).await;

        let page = result.unwrap();
        assert_eq!(
            page.memories
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["m2"]
        );
        assert!(page.has_next_page);
    }

    #[tokio::test]
    async fn maps_paged_access_failures_like_the_overview_request() {
        let (forbidden, _) = get_memories_page_from(1, 2, "403 Forbidden", "{}").await;
        assert!(forbidden
            .unwrap_err()
            .starts_with("memories_permission_denied:"));
        let (failed, _) = get_memories_page_from(1, 2, "500 Internal Server Error", "x").await;
        assert_eq!(failed.unwrap_err(), "memories_unavailable");
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
            {"id":"u","type":"holiday_special","assets":[{"id":"u-1","type":"IMAGE","fileCreatedAt":"2017-01-01T10:00:00.000Z"}]},
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
    }

    #[tokio::test]
    async fn picks_a_cover_that_appears_in_the_timeline_and_drops_memories_with_no_dated_asset() {
        let body = r#"[
            {"id":"mixed","type":"on_this_day","assets":[
                {"id":"dateless","type":"IMAGE"},
                {"id":"dated","type":"IMAGE","fileCreatedAt":"2020-11-15T10:00:00.000Z"}]},
            {"id":"nodates","type":"on_this_day","assets":[{"id":"x","type":"IMAGE"}]}]"#;

        let (result, _) = get_memories_from("200 OK", body).await;

        let memories = result.unwrap();
        assert_eq!(
            memories.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["mixed"]
        );
        assert_eq!(memories[0].cover_asset_id, "dated");
        assert!(memories[0]
            .timeline
            .assets_by_bucket
            .values()
            .flatten()
            .any(|asset| asset.id == memories[0].cover_asset_id));
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
            "memories_permission_denied:configure an API key with the memory.read permission"
        );

        let (failed, _) =
            get_memories_from("500 Internal Server Error", "boom memory-api-key").await;
        assert_eq!(failed.unwrap_err(), "memories_unavailable");

        let (malformed, _) = get_memories_from("200 OK", "{\"not\":\"an array\"}").await;
        assert_eq!(malformed.unwrap_err(), "memories_invalid_response");
    }
}
