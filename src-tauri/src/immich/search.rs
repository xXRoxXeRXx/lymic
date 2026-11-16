//! Search over the Immich Library (ADR-0008, ADR-0009).
//!
//! Search is read-only and sends only Immich's structured `filter` (Immich 3.2 or later).
//! A chronological Search pages through `POST /api/search/metadata` with cursors, keeps at
//! most the [`SEARCH_RESULT_LIMIT`] newest matches and groups them into Timeline Buckets
//! like an Album Timeline.

use super::client::ImmichClient;
use super::response::{read_response_body_limited, request_failure_code};
use super::timeline::{
    group_album_timeline_assets, timeline_asset_from_response, AlbumTimeline, TimelineAssetResponse,
};
use super::TimelineBucket;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

/// The most matches a chronological Search loads (ADR-0009). A constant, not a setting.
pub(crate) const SEARCH_RESULT_LIMIT: usize = 5_000;
/// Immich's largest metadata search page.
const SEARCH_PAGE_SIZE: usize = 1_000;
/// A page of 1 000 Remote Assets with EXIF data is larger than the default response limit.
const MAX_SEARCH_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// The oldest Immich version that understands the structured `filter` (ADR-0008).
const MINIMUM_VERSION: (u64, u64) = (3, 2);
/// What the frontend shows as the required version.
const MINIMUM_VERSION_LABEL: &str = "3.2";
/// The API key permission a Search needs, named in the permission-denied error.
const SEARCH_PERMISSION: &str = "asset.read";

/// How Immich interprets the text of a Search. More Search Modes are added here, together
/// with their arm in [`text_condition`].
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SearchMode {
    Filename,
}

/// The Search Filters of a Search. Every filter is one optional field here and one line in
/// [`SearchFilters::apply`]; all set filters combine with AND.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SearchFilters {}

impl SearchFilters {
    /// Adds the condition of every set filter to the structured `filter` object.
    fn apply(&self, _filter: &mut Map<String, Value>) {}
}

/// One Search as the frontend describes it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchQuery {
    pub text: String,
    pub mode: SearchMode,
    #[serde(default)]
    pub filters: SearchFilters,
}

/// Chronological Search Results: the shape of an Album Timeline plus whether the Search was
/// cut at the result limit.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchTimeline {
    pub buckets: Vec<TimelineBucket>,
    pub assets_by_bucket: BTreeMap<String, Vec<super::TimelineAsset>>,
    /// True when more matches exist than the limit allows, so only the newest are shown.
    pub truncated: bool,
}

/// Whether the connected Immich Server can answer a Search.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchSupport {
    pub supported: bool,
    /// The server's version as `major.minor.patch`.
    pub version: String,
    /// The minimum version a Search requires, for the "unsupported" message.
    pub required_version: String,
}

#[derive(Deserialize)]
struct ServerVersionResponse {
    major: u64,
    minor: u64,
    patch: u64,
}

#[derive(Deserialize)]
struct SearchResponse {
    assets: SearchAssets,
}

#[derive(Deserialize)]
struct SearchAssets {
    #[serde(default)]
    items: Vec<TimelineAssetResponse>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<String>,
}

/// Escapes `\`, `%` and `_` so Immich's `like` matches them literally.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The filter key and condition the text of a Search adds in `mode`.
fn text_condition(mode: SearchMode, text: &str) -> (&'static str, Value) {
    match mode {
        SearchMode::Filename => (
            "originalFileName",
            json!({ "like": format!("%{}%", escape_like(text)) }),
        ),
    }
}

/// Builds Immich's structured `filter` for a Search. No text means a filters-only Search.
fn build_filter(query: &SearchQuery) -> Value {
    let mut filter = Map::new();
    let text = query.text.trim();
    if !text.is_empty() {
        let (key, condition) = text_condition(query.mode, text);
        filter.insert(key.to_string(), condition);
    }
    query.filters.apply(&mut filter);
    Value::Object(filter)
}

fn invalid_response() -> String {
    "search_invalid_response".to_string()
}

impl ImmichClient {
    /// Reads the server's version and reports whether it can answer a Search (ADR-0008).
    /// Errors carry a stable `search_*` code and never the response body.
    pub async fn get_search_support(&self) -> Result<SearchSupport, String> {
        let response = self
            .request(Method::GET, "server/version")
            .send()
            .await
            .map_err(|_| "search_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(request_failure_code(
                "search",
                response.status(),
                SEARCH_PERMISSION,
            ));
        }
        let body = read_response_body_limited(response, 64 * 1024)
            .await
            .map_err(|_| invalid_response())?;
        let version: ServerVersionResponse =
            serde_json::from_slice(&body).map_err(|_| invalid_response())?;
        Ok(SearchSupport {
            supported: (version.major, version.minor) >= MINIMUM_VERSION,
            version: format!("{}.{}.{}", version.major, version.minor, version.patch),
            required_version: MINIMUM_VERSION_LABEL.to_string(),
        })
    }

    /// Runs a chronological Search: every page up to [`SEARCH_RESULT_LIMIT`] newest matches,
    /// grouped by local capture month. Any failed page fails the whole load.
    pub async fn search_assets(&self, query: &SearchQuery) -> Result<SearchTimeline, String> {
        let filter = build_filter(query);
        let mut cursor: Option<String> = None;
        let mut seen_cursors = HashSet::new();
        let mut assets = Vec::new();
        let mut truncated = false;

        loop {
            let mut body = json!({
                "filter": filter,
                "orderBy": { "field": "localDateTime", "direction": "desc" },
                "size": SEARCH_PAGE_SIZE,
                "withExif": true,
            });
            if let Some(cursor) = &cursor {
                body["cursor"] = cursor.clone().into();
            }
            let response = self
                .request(Method::POST, "search/metadata")
                .json(&body)
                .send()
                .await
                .map_err(|_| "search_unavailable".to_string())?;
            if !response.status().is_success() {
                return Err(request_failure_code(
                    "search",
                    response.status(),
                    SEARCH_PERMISSION,
                ));
            }
            let body = read_response_body_limited(response, MAX_SEARCH_RESPONSE_BYTES)
                .await
                .map_err(|_| invalid_response())?;
            let page: SearchResponse =
                serde_json::from_slice(&body).map_err(|_| invalid_response())?;
            assets.extend(page.assets.items);

            let next_cursor = page.assets.next_cursor.filter(|cursor| !cursor.is_empty());
            if assets.len() >= SEARCH_RESULT_LIMIT {
                truncated = assets.len() > SEARCH_RESULT_LIMIT || next_cursor.is_some();
                assets.truncate(SEARCH_RESULT_LIMIT);
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            // A cursor that was already used would loop forever.
            if !seen_cursors.insert(next_cursor.clone()) {
                return Err(invalid_response());
            }
            cursor = Some(next_cursor);
        }

        let AlbumTimeline {
            buckets,
            assets_by_bucket,
        } = group_album_timeline_assets(
            assets
                .into_iter()
                .map(|asset| timeline_asset_from_response(asset, false))
                .collect(),
        );
        Ok(SearchTimeline {
            buckets,
            assets_by_bucket,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchFilters, SearchMode, SearchQuery, SearchSupport, SearchTimeline};
    use crate::immich::test_support::{serve_once, serve_pages, test_client};
    use serde_json::{json, Value};
    use std::time::Duration;

    fn filename_query(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.to_string(),
            mode: SearchMode::Filename,
            filters: SearchFilters::default(),
        }
    }

    fn json_body(request: &str) -> Value {
        let body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("request has a body");
        serde_json::from_str(body).expect("request body is JSON")
    }

    fn asset(id: &str, taken_at: &str) -> Value {
        json!({ "id": id, "type": "IMAGE", "exifInfo": { "dateTimeOriginal": taken_at } })
    }

    fn page(items: Vec<Value>, next_cursor: Option<&str>) -> String {
        json!({ "assets": { "items": items, "nextCursor": next_cursor } }).to_string()
    }

    /// Runs one Search against the given canned responses; returns the result and the requests.
    async fn search(
        responses: Vec<(&'static str, String)>,
        query: SearchQuery,
    ) -> (Result<SearchTimeline, String>, Vec<String>) {
        let (url, server) = serve_pages(responses).await;
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            test_client(url, "test-key").search_assets(&query),
        )
        .await
        .expect("the Search must not request more pages than the server has");
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn a_filename_search_posts_only_the_structured_filter_and_groups_the_newest_first() {
        let first = page(
            vec![
                asset("older", "2025-02-10T10:00:00.000Z"),
                asset("newer", "2025-03-01T10:00:00.000Z"),
            ],
            None,
        );
        let (result, requests) = search(vec![("200 OK", first)], filename_query("IMG")).await;

        let request = &requests[0];
        assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert_eq!(
            json_body(request),
            json!({
                "filter": { "originalFileName": { "like": "%IMG%" } },
                "orderBy": { "field": "localDateTime", "direction": "desc" },
                "size": 1000,
                "withExif": true,
            })
        );
        let timeline = result.unwrap();
        let buckets: Vec<_> = timeline
            .buckets
            .iter()
            .map(|bucket| bucket.time_bucket.as_str())
            .collect();
        assert_eq!(
            buckets,
            vec!["2025-03-01T00:00:00.000Z", "2025-02-01T00:00:00.000Z"]
        );
        assert_eq!(
            timeline.assets_by_bucket["2025-03-01T00:00:00.000Z"][0].id,
            "newer"
        );
        assert!(!timeline.truncated);
    }

    #[tokio::test]
    async fn percent_underscore_and_backslash_in_the_text_are_searched_literally() {
        let empty = page(vec![], None);
        let (_, requests) = search(vec![("200 OK", empty)], filename_query(r"100%_a\b")).await;

        assert_eq!(
            json_body(&requests[0])["filter"]["originalFileName"]["like"],
            json!(r"%100\%\_a\\b%")
        );
    }

    #[tokio::test]
    async fn the_text_is_trimmed_and_an_empty_text_sends_no_text_condition() {
        let empty = page(vec![], None);
        let (_, requests) = search(vec![("200 OK", empty)], filename_query("  IMG ")).await;
        assert_eq!(
            json_body(&requests[0])["filter"]["originalFileName"]["like"],
            json!("%IMG%")
        );

        let (_, requests) =
            search(vec![("200 OK", page(vec![], None))], filename_query("  ")).await;
        assert_eq!(json_body(&requests[0])["filter"], json!({}));
    }

    #[tokio::test]
    async fn later_pages_are_requested_with_the_cursor_the_server_returned() {
        let first = page(
            vec![asset("a", "2025-03-01T10:00:00.000Z")],
            Some("cursor-1"),
        );
        let second = page(vec![asset("b", "2025-02-01T10:00:00.000Z")], None);
        let (result, requests) = search(
            vec![("200 OK", first), ("200 OK", second)],
            filename_query("x"),
        )
        .await;

        assert_eq!(requests.len(), 2);
        assert!(json_body(&requests[0]).get("cursor").is_none());
        assert_eq!(json_body(&requests[1])["cursor"], json!("cursor-1"));
        assert_eq!(result.unwrap().buckets.len(), 2);
    }

    #[tokio::test]
    async fn assets_without_a_usable_capture_time_are_omitted() {
        let body = page(
            vec![
                asset("dated", "2025-03-01T10:00:00.000Z"),
                json!({ "id": "undated", "type": "IMAGE", "exifInfo": {} }),
            ],
            None,
        );
        let (result, _) = search(vec![("200 OK", body)], filename_query("x")).await;

        let timeline = result.unwrap();
        let ids: Vec<_> = timeline
            .assets_by_bucket
            .values()
            .flatten()
            .map(|asset| asset.id.as_str())
            .collect();
        assert_eq!(ids, vec!["dated"]);
    }

    fn full_page(prefix: &str, next_cursor: Option<&str>) -> String {
        let items = (0..1_000)
            .map(|index| asset(&format!("{prefix}-{index}"), "2025-03-01T10:00:00.000Z"))
            .collect();
        page(items, next_cursor)
    }

    #[tokio::test]
    async fn a_search_stops_at_the_5000_newest_matches_and_reports_that_it_was_cut() {
        let pages = (0..5)
            .map(|index| {
                (
                    "200 OK",
                    full_page(&format!("p{index}"), Some(&format!("c{index}"))),
                )
            })
            .collect();
        let (result, requests) = search(pages, filename_query("x")).await;

        assert_eq!(requests.len(), 5);
        let timeline = result.unwrap();
        assert!(timeline.truncated);
        assert_eq!(timeline.assets_by_bucket.values().flatten().count(), 5_000);
    }

    #[tokio::test]
    async fn a_result_of_exactly_the_limit_is_not_reported_as_cut() {
        let mut pages: Vec<_> = (0..4)
            .map(|index| {
                (
                    "200 OK",
                    full_page(&format!("p{index}"), Some(&format!("c{index}"))),
                )
            })
            .collect();
        pages.push(("200 OK", full_page("last", None)));
        let (result, _) = search(pages, filename_query("x")).await;

        let timeline = result.unwrap();
        assert!(!timeline.truncated);
        assert_eq!(timeline.assets_by_bucket.values().flatten().count(), 5_000);
    }

    #[tokio::test]
    async fn a_failed_page_fails_the_whole_search() {
        let first = page(
            vec![asset("a", "2025-03-01T10:00:00.000Z")],
            Some("cursor-1"),
        );
        let (result, _) = search(
            vec![
                ("200 OK", first),
                ("500 Internal Server Error", "secret".into()),
            ],
            filename_query("x"),
        )
        .await;

        assert_eq!(result.unwrap_err(), "search_unavailable");
    }

    #[tokio::test]
    async fn a_next_page_that_does_not_advance_is_rejected() {
        let first = page(
            vec![asset("a", "2025-03-01T10:00:00.000Z")],
            Some("cursor-1"),
        );
        let repeat = page(
            vec![asset("b", "2025-02-01T10:00:00.000Z")],
            Some("cursor-1"),
        );
        let (result, requests) = search(
            vec![("200 OK", first), ("200 OK", repeat)],
            filename_query("x"),
        )
        .await;

        assert_eq!(requests.len(), 2);
        assert_eq!(result.unwrap_err(), "search_invalid_response");
    }

    #[tokio::test]
    async fn failures_become_credential_free_codes_without_the_response_body() {
        let cases = [
            ("401 Unauthorized", "search_authentication_required"),
            ("403 Forbidden", "search_permission_denied:asset.read"),
            ("500 Internal Server Error", "search_unavailable"),
            ("400 Bad Request", "search_unavailable"),
        ];
        for (status, expected) in cases {
            let (result, _) =
                search(vec![(status, "secret-key".into())], filename_query("x")).await;
            assert_eq!(result.unwrap_err(), expected, "{status}");
        }
        let (result, _) = search(vec![("200 OK", "{secret".into())], filename_query("x")).await;
        assert_eq!(result.unwrap_err(), "search_invalid_response");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_unavailable() {
        let (url, server) = serve_pages(Vec::<(String, String)>::new()).await;
        server.await.unwrap();
        let result = test_client(url, "test-key")
            .search_assets(&filename_query("x"))
            .await;

        assert_eq!(result.unwrap_err(), "search_unavailable");
    }

    async fn support_for(version: &str) -> (SearchSupport, String) {
        let (url, request) = serve_once("200 OK", version).await;
        let support = test_client(url, "test-key")
            .get_search_support()
            .await
            .unwrap();
        (support, request.await.unwrap())
    }

    #[tokio::test]
    async fn the_server_version_decides_whether_search_is_supported() {
        let (support, request) =
            support_for(r#"{"major":3,"minor":2,"patch":0,"prerelease":null,"featureLevel":1}"#)
                .await;
        assert!(request.starts_with("GET /api/server/version HTTP/1.1"));
        assert_eq!(
            support,
            SearchSupport {
                supported: true,
                version: "3.2.0".into(),
                required_version: "3.2".into(),
            }
        );
        for (version, supported) in [
            (r#"{"major":3,"minor":4,"patch":1}"#, true),
            (r#"{"major":4,"minor":0,"patch":0}"#, true),
            (r#"{"major":3,"minor":1,"patch":9}"#, false),
            (r#"{"major":2,"minor":9,"patch":0}"#, false),
        ] {
            assert_eq!(
                support_for(version).await.0.supported,
                supported,
                "{version}"
            );
        }
    }

    #[tokio::test]
    async fn version_failures_are_credential_free_codes() {
        for (status, body, expected) in [
            ("503 Service Unavailable", "secret", "search_unavailable"),
            ("200 OK", "{secret", "search_invalid_response"),
        ] {
            let (url, _server) = serve_once(status, body).await;
            let error = test_client(url, "test-key")
                .get_search_support()
                .await
                .unwrap_err();
            assert_eq!(error, expected);
        }
    }

    #[test]
    fn a_search_timeline_serializes_in_camel_case() {
        let timeline = SearchTimeline {
            buckets: vec![],
            assets_by_bucket: Default::default(),
            truncated: true,
        };
        assert_eq!(
            serde_json::to_string(&timeline).unwrap(),
            r#"{"buckets":[],"assetsByBucket":{},"truncated":true}"#
        );
    }

    #[test]
    fn a_query_deserializes_from_the_frontend_shape_with_default_filters() {
        let query: SearchQuery =
            serde_json::from_str(r#"{"text":"IMG","mode":"filename"}"#).unwrap();
        assert_eq!(query, filename_query("IMG"));
        let query: SearchQuery =
            serde_json::from_str(r#"{"text":"IMG","mode":"filename","filters":{}}"#).unwrap();
        assert_eq!(query, filename_query("IMG"));
    }
}
