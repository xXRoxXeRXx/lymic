//! Search over the Immich Library (ADR-0008, ADR-0009).
//!
//! Search is read-only and sends only Immich's structured `filter` (Immich 3.2 or later).
//! A chronological Search pages through `POST /api/search/metadata` with cursors, keeps at
//! most the [`SEARCH_RESULT_LIMIT`] newest matches and groups them into Timeline Buckets
//! like an Album Timeline.

use super::client::ImmichClient;
use super::response::{read_response_body_limited, request_failure_code};
use super::timeline::{
    deserialize_next_page, group_album_timeline_assets, timeline_asset_from_response,
    AlbumTimeline, TimelineAssetResponse,
};
use super::{TimelineAsset, TimelineBucket};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

/// The most matches a chronological Search loads (ADR-0009). A constant, not a setting.
pub(crate) const SEARCH_RESULT_LIMIT: usize = 5_000;
/// Context Search Results per page (ADR-0009); a constant, not a setting.
const CONTEXT_PAGE_SIZE: usize = 100;
/// Immich's largest metadata search page.
const SEARCH_PAGE_SIZE: usize = 1_000;
/// A page of 1 000 Remote Assets with EXIF data is larger than the default response limit.
const MAX_SEARCH_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// The oldest Immich version that understands the structured `filter` (ADR-0008).
const MINIMUM_VERSION: (u64, u64) = (3, 2);
/// Which optional Search Modes the connected Immich Server can serve, read from
/// `GET /api/server/features`. A flag the server does not report counts as switched off.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchFeatures {
    /// Smart Search (needed by the Context Search Mode).
    pub smart_search: bool,
    /// Recognized text (needed by the OCR Search Mode).
    pub ocr: bool,
}

#[derive(Deserialize)]
struct ServerFeaturesResponse {
    #[serde(rename = "smartSearch", default)]
    smart_search: bool,
    #[serde(default)]
    ocr: bool,
}

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
    Description,
    FolderPath,
    /// Text recognized within the media; available only while the server's OCR feature is on.
    Ocr,
    /// What a photo shows, matched by Immich's smart search and ordered by relevance (ADR-0009). Needs a
    /// text: without one the Search runs as a chronological filters-only Search. Available only while the
    /// server's Smart Search feature is on.
    Context,
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

/// One page of Context Search Results in the order of relevance Immich returned them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextSearchPage {
    pub assets: Vec<TimelineAsset>,
    /// The page to request next, or none when this was the last page.
    pub next_page: Option<u64>,
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

#[derive(Deserialize)]
struct ContextSearchResponse {
    assets: ContextSearchAssets,
}

#[derive(Deserialize)]
struct ContextSearchAssets {
    #[serde(default)]
    items: Vec<TimelineAssetResponse>,
    #[serde(
        rename = "nextPage",
        default,
        deserialize_with = "deserialize_next_page"
    )]
    next_page: Option<u64>,
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

/// The filter key and condition the text of a Search adds in `mode`. Context has none: its text is
/// the `query` of a smart search, not a condition of the structured filter.
fn text_condition(mode: SearchMode, text: &str) -> Option<(&'static str, Value)> {
    let contains = || json!({ "like": format!("%{}%", escape_like(text)) });
    Some(match mode {
        SearchMode::Filename => ("originalFileName", contains()),
        SearchMode::Description => ("description", contains()),
        SearchMode::FolderPath => ("originalPath", contains()),
        // Recognized text is matched by similarity, which has no wildcards to escape.
        SearchMode::Ocr => ("ocr", json!({ "matches": text })),
        SearchMode::Context => return None,
    })
}

/// Builds Immich's structured `filter` for a Search. No text means a filters-only Search.
fn build_filter(query: &SearchQuery) -> Value {
    let mut filter = Map::new();
    let text = query.text.trim();
    if let Some((key, condition)) = (!text.is_empty())
        .then(|| text_condition(query.mode, text))
        .flatten()
    {
        filter.insert(key.to_string(), condition);
    }
    query.filters.apply(&mut filter);
    Value::Object(filter)
}

fn invalid_response() -> String {
    "search_invalid_response".to_string()
}

/// A Search the frontend should never have sent (a Context text sent to the wrong endpoint).
fn invalid_request() -> String {
    "search_invalid_request".to_string()
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

    /// Reads which optional Search Modes the server can serve. Errors carry a stable
    /// `search_*` code and never the response body.
    pub async fn get_search_features(&self) -> Result<SearchFeatures, String> {
        let response = self
            .request(Method::GET, "server/features")
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
        let features: ServerFeaturesResponse =
            serde_json::from_slice(&body).map_err(|_| invalid_response())?;
        Ok(SearchFeatures {
            smart_search: features.smart_search,
            ocr: features.ocr,
        })
    }

    /// Runs a chronological Search: every page up to [`SEARCH_RESULT_LIMIT`] newest matches,
    /// grouped by local capture month. Any failed page fails the whole load.
    pub async fn search_assets(&self, query: &SearchQuery) -> Result<SearchTimeline, String> {
        // A Context text is answered by `search_context_page`, never by the chronological Search.
        if query.mode == SearchMode::Context && !query.text.trim().is_empty() {
            return Err(invalid_request());
        }
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

impl ImmichClient {
    /// Runs one page of a Context Search: Immich's smart search with the text and the structured
    /// filter, in the order of relevance. The caller pages with the `next_page` it is given.
    pub async fn search_context_page(
        &self,
        query: &SearchQuery,
        page: u64,
    ) -> Result<ContextSearchPage, String> {
        let text = query.text.trim();
        if query.mode != SearchMode::Context || text.is_empty() {
            return Err(invalid_request());
        }
        let body = json!({
            "query": text,
            "filter": build_filter(query),
            "page": page,
            "size": CONTEXT_PAGE_SIZE,
            "withExif": true,
        });
        let response = self
            .request(Method::POST, "search/smart")
            .json(&body)
            .send()
            .await
            .map_err(|_| "search_unavailable".to_string())?;
        let status = response.status();
        if status == StatusCode::BAD_REQUEST {
            // The server rejects smart search although its features reported it on.
            return Err("search_context_unavailable".to_string());
        }
        if !status.is_success() {
            return Err(request_failure_code("search", status, SEARCH_PERMISSION));
        }
        let body = read_response_body_limited(response, MAX_SEARCH_RESPONSE_BYTES)
            .await
            .map_err(|_| invalid_response())?;
        let payload: ContextSearchResponse =
            serde_json::from_slice(&body).map_err(|_| invalid_response())?;
        if payload.assets.next_page.is_some_and(|next| next <= page) {
            return Err(invalid_response());
        }
        Ok(ContextSearchPage {
            assets: payload
                .assets
                .items
                .into_iter()
                .map(|asset| timeline_asset_from_response(asset, false))
                .collect(),
            next_page: payload.assets.next_page,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContextSearchPage, SearchFeatures, SearchFilters, SearchMode, SearchQuery, SearchSupport,
        SearchTimeline,
    };
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

    fn mode_query(mode: SearchMode, text: &str) -> SearchQuery {
        SearchQuery {
            text: text.to_string(),
            mode,
            filters: SearchFilters::default(),
        }
    }

    #[tokio::test]
    async fn each_search_mode_sends_its_own_text_condition_in_the_structured_filter() {
        let cases = [
            (
                SearchMode::Filename,
                json!({ "originalFileName": { "like": "%holiday%" } }),
            ),
            (
                SearchMode::Description,
                json!({ "description": { "like": "%holiday%" } }),
            ),
            (
                SearchMode::FolderPath,
                json!({ "originalPath": { "like": "%holiday%" } }),
            ),
            // Recognized text is matched by similarity, not by `like`.
            (SearchMode::Ocr, json!({ "ocr": { "matches": "holiday" } })),
        ];
        for (mode, expected) in cases {
            let (result, requests) = search(
                vec![("200 OK", page(vec![], None))],
                mode_query(mode, "holiday"),
            )
            .await;

            assert!(result.is_ok(), "{mode:?}");
            let request = &requests[0];
            assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
            assert_eq!(json_body(request)["filter"], expected, "{mode:?}");
            assert!(json_body(request).get("query").is_none(), "{mode:?}");
        }
    }

    #[tokio::test]
    async fn description_and_folder_path_escape_like_wildcards_but_ocr_text_is_sent_as_typed() {
        for (mode, key) in [
            (SearchMode::Description, "description"),
            (SearchMode::FolderPath, "originalPath"),
        ] {
            let (_, requests) = search(
                vec![("200 OK", page(vec![], None))],
                mode_query(mode, r"100%_a\b"),
            )
            .await;
            assert_eq!(
                json_body(&requests[0])["filter"][key]["like"],
                json!(r"%100\%\_a\\b%"),
                "{mode:?}"
            );
        }
        let (_, requests) = search(
            vec![("200 OK", page(vec![], None))],
            mode_query(SearchMode::Ocr, "100%_a"),
        )
        .await;
        assert_eq!(
            json_body(&requests[0])["filter"],
            json!({ "ocr": { "matches": "100%_a" } })
        );
    }

    #[tokio::test]
    async fn a_filters_only_search_sends_no_text_condition_in_any_mode() {
        for mode in [
            SearchMode::Description,
            SearchMode::FolderPath,
            SearchMode::Ocr,
        ] {
            let (_, requests) =
                search(vec![("200 OK", page(vec![], None))], mode_query(mode, " ")).await;
            assert_eq!(json_body(&requests[0])["filter"], json!({}), "{mode:?}");
        }
    }

    #[test]
    fn search_modes_deserialize_from_the_frontend_names() {
        for (name, mode) in [
            ("filename", SearchMode::Filename),
            ("description", SearchMode::Description),
            ("folderPath", SearchMode::FolderPath),
            ("ocr", SearchMode::Ocr),
            ("context", SearchMode::Context),
        ] {
            let query: SearchQuery =
                serde_json::from_str(&format!(r#"{{"text":"x","mode":"{name}"}}"#)).unwrap();
            assert_eq!(query.mode, mode);
        }
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

    #[tokio::test]
    async fn the_server_features_say_whether_smart_search_and_ocr_are_switched_on() {
        for (body, smart_search, ocr) in [
            (r#"{"smartSearch":true,"ocr":true,"map":true}"#, true, true),
            (r#"{"smartSearch":true,"ocr":false}"#, true, false),
            (r#"{"smartSearch":false,"ocr":true}"#, false, true),
        ] {
            let (url, request) = serve_once("200 OK", body).await;
            let features = test_client(url, "test-key")
                .get_search_features()
                .await
                .unwrap();
            let request = request.await.unwrap();

            assert!(request.starts_with("GET /api/server/features HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            assert_eq!(features, SearchFeatures { smart_search, ocr });
        }
    }

    #[tokio::test]
    async fn a_missing_feature_flag_counts_as_switched_off() {
        let (url, _server) = serve_once("200 OK", r#"{"map":true}"#).await;
        let features = test_client(url, "test-key")
            .get_search_features()
            .await
            .unwrap();
        assert_eq!(
            features,
            SearchFeatures {
                smart_search: false,
                ocr: false
            }
        );
    }

    #[tokio::test]
    async fn feature_failures_are_credential_free_codes() {
        for (status, body, expected) in [
            (
                "401 Unauthorized",
                "secret",
                "search_authentication_required",
            ),
            ("503 Service Unavailable", "secret", "search_unavailable"),
            ("200 OK", "{secret", "search_invalid_response"),
        ] {
            let (url, _server) = serve_once(status, body).await;
            let error = test_client(url, "test-key")
                .get_search_features()
                .await
                .unwrap_err();
            assert_eq!(error, expected);
        }
    }

    #[test]
    fn search_features_serialize_in_camel_case() {
        let features = SearchFeatures {
            smart_search: true,
            ocr: false,
        };
        assert_eq!(
            serde_json::to_string(&features).unwrap(),
            r#"{"smartSearch":true,"ocr":false}"#
        );
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

    #[tokio::test]
    async fn a_context_search_without_text_runs_as_a_chronological_filters_only_search() {
        let (result, requests) = search(
            vec![(
                "200 OK",
                page(vec![asset("a", "2025-03-01T10:00:00.000Z")], None),
            )],
            mode_query(SearchMode::Context, "  "),
        )
        .await;

        assert!(result.is_ok());
        let request = &requests[0];
        assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
        assert_eq!(json_body(request)["filter"], json!({}));
        assert!(json_body(request).get("query").is_none());
    }

    #[tokio::test]
    async fn a_context_text_is_never_sent_to_the_chronological_search() {
        let (result, requests) = search(Vec::new(), mode_query(SearchMode::Context, "dog")).await;

        assert_eq!(result.unwrap_err(), "search_invalid_request");
        assert!(requests.is_empty());
    }

    fn smart_page(items: Vec<Value>, next_page: Option<&str>) -> String {
        json!({ "assets": { "items": items, "nextPage": next_page } }).to_string()
    }

    /// Runs one smart search page against the given canned response.
    async fn context_page(
        status: &'static str,
        body: String,
        query: SearchQuery,
        page: u64,
    ) -> (Result<ContextSearchPage, String>, Vec<String>) {
        let (url, server) = serve_pages(vec![(status, body)]).await;
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            test_client(url, "test-key").search_context_page(&query, page),
        )
        .await
        .expect("a Context Search page is one request");
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn a_context_search_posts_the_text_and_the_structured_filter_to_smart_search() {
        let body = smart_page(
            vec![
                asset("best", "2025-01-01T10:00:00.000Z"),
                asset("second", "2025-03-01T10:00:00.000Z"),
                json!({ "id": "undated", "type": "VIDEO", "exifInfo": {} }),
            ],
            Some("2"),
        );
        let (result, requests) = context_page(
            "200 OK",
            body,
            mode_query(SearchMode::Context, " dog on a beach "),
            1,
        )
        .await;

        let request = &requests[0];
        assert!(request.starts_with("POST /api/search/smart HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert_eq!(
            json_body(request),
            json!({
                "query": "dog on a beach",
                "filter": {},
                "page": 1,
                "size": 100,
                "withExif": true,
            })
        );
        let page = result.unwrap();
        // The server's relevance order is kept and nothing needs a capture time.
        let ids: Vec<_> = page.assets.iter().map(|asset| asset.id.as_str()).collect();
        assert_eq!(ids, vec!["best", "second", "undated"]);
        assert_eq!(page.next_page, Some(2));
    }

    #[tokio::test]
    async fn a_later_context_page_asks_for_the_page_the_server_announced() {
        let (result, requests) = context_page(
            "200 OK",
            smart_page(vec![asset("c", "2025-03-01T10:00:00.000Z")], None),
            mode_query(SearchMode::Context, "dog"),
            3,
        )
        .await;

        assert_eq!(json_body(&requests[0])["page"], json!(3));
        assert_eq!(result.unwrap().next_page, None);
    }

    #[tokio::test]
    async fn a_context_search_needs_a_text() {
        let (result, requests) = context_page(
            "200 OK",
            smart_page(vec![], None),
            mode_query(SearchMode::Context, "  "),
            1,
        )
        .await;

        assert_eq!(result.unwrap_err(), "search_invalid_request");
        assert!(requests.is_empty());
    }

    #[tokio::test]
    async fn a_next_page_that_does_not_advance_is_rejected_by_a_context_search() {
        let (result, _) = context_page(
            "200 OK",
            smart_page(vec![asset("a", "2025-03-01T10:00:00.000Z")], Some("2")),
            mode_query(SearchMode::Context, "dog"),
            2,
        )
        .await;

        assert_eq!(result.unwrap_err(), "search_invalid_response");
    }

    #[tokio::test]
    async fn context_search_failures_become_credential_free_codes_without_the_response_body() {
        let cases = [
            ("401 Unauthorized", "search_authentication_required"),
            ("403 Forbidden", "search_permission_denied:asset.read"),
            ("500 Internal Server Error", "search_unavailable"),
            // The server rejects smart search although the feature was reported on.
            ("400 Bad Request", "search_context_unavailable"),
        ];
        for (status, expected) in cases {
            let (result, _) = context_page(
                status,
                "secret-key".into(),
                mode_query(SearchMode::Context, "dog"),
                1,
            )
            .await;
            assert_eq!(result.unwrap_err(), expected, "{status}");
        }
        let (result, _) = context_page(
            "200 OK",
            "{secret".into(),
            mode_query(SearchMode::Context, "dog"),
            1,
        )
        .await;
        assert_eq!(result.unwrap_err(), "search_invalid_response");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_unavailable_for_a_context_search() {
        let (url, server) = serve_pages(Vec::<(String, String)>::new()).await;
        server.await.unwrap();
        let result = test_client(url, "test-key")
            .search_context_page(&mode_query(SearchMode::Context, "dog"), 1)
            .await;

        assert_eq!(result.unwrap_err(), "search_unavailable");
    }

    #[tokio::test]
    async fn a_context_page_larger_than_the_default_response_limit_is_accepted() {
        let padding = "x".repeat(2 * 1024 * 1024);
        let mut big = asset("a", "2025-03-01T10:00:00.000Z");
        big["originalFileName"] = json!(padding);
        let (result, _) = context_page(
            "200 OK",
            smart_page(vec![big], None),
            mode_query(SearchMode::Context, "dog"),
            1,
        )
        .await;

        assert_eq!(result.unwrap().assets.len(), 1);
    }

    #[test]
    fn a_context_page_serializes_in_camel_case() {
        let page = ContextSearchPage {
            assets: vec![],
            next_page: Some(2),
        };
        assert_eq!(
            serde_json::to_string(&page).unwrap(),
            r#"{"assets":[],"nextPage":2}"#
        );
    }
}
