//! Search over the Immich Library (ADR-0008, ADR-0009).
//!
//! Search is read-only and sends only Immich's structured `filter` (Immich 3.2 or later).
//! A chronological Search pages through `POST /api/search/metadata` with cursors, keeps at
//! most the [`SEARCH_RESULT_LIMIT`] newest results and groups them into Timeline Buckets
//! like an Album Timeline.

use super::client::ImmichClient;
use super::response::{read_response_body_limited, request_failure_code};
use super::search_filters::{
    apply_favorite, apply_not_trashed, apply_person_ids, apply_unfiled, apply_visibility,
    CameraFilter, DateRange, LocationFilter, MediaType, Rating,
};
use super::timeline::{
    group_album_timeline_assets, timeline_asset_from_response, AlbumTimeline, TimelineAssetResponse,
};
use super::{TimelineAsset, TimelineBucket};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

/// The most results a chronological Search loads (ADR-0009). A constant, not a setting.
pub(crate) const SEARCH_RESULT_LIMIT: usize = 5_000;
/// Context Search Results per page (ADR-0009); a constant, not a setting.
const CONTEXT_PAGE_SIZE: usize = 100;
/// The most Context Search Results Immich answers in one request: the structured smart search has no
/// paging, so this is also the most a Context Search shows (ADR-0009).
const CONTEXT_RESULT_LIMIT: usize = 1_000;
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

/// Suggestion lists are short strings, well within this limit.
pub(super) const SUGGESTIONS_MAX_BYTES: usize = 4 * 1024 * 1024;
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
pub(crate) struct SearchFilters {
    pub date: Option<DateRange>,
    pub media_type: Option<MediaType>,
    /// Recognized People that must all appear in a Remote Asset.
    pub person_ids: Option<Vec<String>>,
    /// Display option: only Favorites.
    pub favorite: bool,
    /// Display option: only Archived Assets. Off means Archived Assets are left out.
    pub archived: bool,
    /// Display option: only Unfiled Assets (in no Album).
    pub unfiled: bool,
    pub rating: Option<Rating>,
    pub camera: Option<CameraFilter>,
    pub location: Option<LocationFilter>,
}

impl SearchFilters {
    /// Adds the condition of every set filter to the structured `filter` object.
    fn apply(&self, filter: &mut Map<String, Value>) {
        if let Some(date) = &self.date {
            date.apply(filter);
        }
        if let Some(media_type) = self.media_type {
            media_type.apply(filter);
        }
        if let Some(person_ids) = &self.person_ids {
            apply_person_ids(person_ids, filter);
        }
        if self.favorite {
            apply_favorite(filter);
        }
        if self.unfiled {
            apply_unfiled(filter);
        }
        if let Some(rating) = self.rating {
            rating.apply(filter);
        }
        apply_visibility(self.archived, filter);
        apply_not_trashed(filter);
        if let Some(camera) = &self.camera {
            camera.apply(filter);
        }
        if let Some(location) = &self.location {
            location.apply(filter);
        }
    }
}

/// One Search as the frontend describes it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchRequest {
    pub text: String,
    pub mode: SearchMode,
    #[serde(default)]
    pub filters: SearchFilters,
}

/// Chronological Search Results: the shape of an Album Timeline plus whether the Search was
/// cut at the result limit.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResults {
    pub buckets: Vec<TimelineBucket>,
    pub assets_by_bucket: BTreeMap<String, Vec<super::TimelineAsset>>,
    /// True when more results exist than the limit allows, so only the newest are shown.
    pub truncated: bool,
}

/// One page of Context Search Results in the order of relevance Immich returned them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextSearchPage {
    pub assets: Vec<TimelineAsset>,
    /// The page to request next, or none when this was the last page.
    pub next_page: Option<u64>,
    /// True when this was the last page the limit allows and Immich filled it, so the Search
    /// may have more results than are shown.
    pub limit_reached: bool,
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
fn build_filter(request: &SearchRequest) -> Value {
    let mut filter = Map::new();
    let text = request.text.trim();
    if let Some((key, condition)) = (!text.is_empty())
        .then(|| text_condition(request.mode, text))
        .flatten()
    {
        filter.insert(key.to_string(), condition);
    }
    request.filters.apply(&mut filter);
    Value::Object(filter)
}

fn invalid_response() -> String {
    "search_invalid_response".to_string()
}

/// A Search the frontend should never have sent (a Context text sent to the wrong endpoint).
fn invalid_request() -> String {
    "search_invalid_request".to_string()
}

/// Sends one Search request and parses its JSON response. Errors carry a stable `search_*`
/// code and never the response body.
pub(super) async fn send_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    max_bytes: usize,
) -> Result<T, String> {
    send_json_mapping_statuses(request, max_bytes, StatusCodes::default()).await
}

/// The codes a Search request gives particular statuses instead of the generic ones.
#[derive(Default)]
struct StatusCodes<'a> {
    /// For a `400 Bad Request`.
    bad_request: Option<&'a str>,
    /// For any `5xx` server error.
    server_error: Option<&'a str>,
}

/// Like [`send_json`], with the codes in `codes` for the statuses they name.
async fn send_json_mapping_statuses<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    max_bytes: usize,
    codes: StatusCodes<'_>,
) -> Result<T, String> {
    let response = request
        .send()
        .await
        .map_err(|_| "search_unavailable".to_string())?;
    if let (StatusCode::BAD_REQUEST, Some(code)) = (response.status(), codes.bad_request) {
        return Err(code.to_string());
    }
    if let (true, Some(code)) = (response.status().is_server_error(), codes.server_error) {
        return Err(code.to_string());
    }
    if !response.status().is_success() {
        return Err(request_failure_code(
            "search",
            response.status(),
            SEARCH_PERMISSION,
        ));
    }
    let body = read_response_body_limited(response, max_bytes)
        .await
        .map_err(|_| invalid_response())?;
    serde_json::from_slice(&body).map_err(|_| invalid_response())
}

impl ImmichClient {
    /// Reads the server's version and reports whether it can answer a Search (ADR-0008).
    /// Errors carry a stable `search_*` code and never the response body.
    pub async fn get_search_support(&self) -> Result<SearchSupport, String> {
        let version: ServerVersionResponse =
            send_json(self.request(Method::GET, "server/version"), 64 * 1024).await?;
        Ok(SearchSupport {
            supported: (version.major, version.minor) >= MINIMUM_VERSION,
            version: format!("{}.{}.{}", version.major, version.minor, version.patch),
            required_version: MINIMUM_VERSION_LABEL.to_string(),
        })
    }

    /// Reads which optional Search Modes the server can serve. Errors carry a stable
    /// `search_*` code and never the response body.
    pub async fn get_search_features(&self) -> Result<SearchFeatures, String> {
        let features: ServerFeaturesResponse =
            send_json(self.request(Method::GET, "server/features"), 64 * 1024).await?;
        Ok(SearchFeatures {
            smart_search: features.smart_search,
            ocr: features.ocr,
        })
    }

    /// Runs a chronological Search: every page up to [`SEARCH_RESULT_LIMIT`] newest results,
    /// grouped by local capture month. Any failed page fails the whole load.
    pub async fn search_assets(&self, request: &SearchRequest) -> Result<SearchResults, String> {
        // A Context text is answered by `search_context_page`, never by the chronological Search.
        if request.mode == SearchMode::Context && !request.text.trim().is_empty() {
            return Err(invalid_request());
        }
        let filter = build_filter(request);
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
            let page: SearchResponse = send_json(
                self.request(Method::POST, "search/metadata").json(&body),
                MAX_SEARCH_RESPONSE_BYTES,
            )
            .await?;
            let page_is_empty = page.assets.items.is_empty();
            assets.extend(page.assets.items);

            let next_cursor = page.assets.next_cursor.filter(|cursor| !cursor.is_empty());
            if let Some(next_cursor) = &next_cursor {
                // A cursor that was already used, or a page without assets that promises more,
                // would loop forever, also on the page that reaches the limit.
                if page_is_empty || !seen_cursors.insert(next_cursor.clone()) {
                    return Err(invalid_response());
                }
            }
            if assets.len() >= SEARCH_RESULT_LIMIT {
                truncated = assets.len() > SEARCH_RESULT_LIMIT || next_cursor.is_some();
                assets.truncate(SEARCH_RESULT_LIMIT);
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
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
        Ok(SearchResults {
            buckets,
            assets_by_bucket,
            truncated,
        })
    }
}

impl ImmichClient {
    /// Runs one page of a Context Search: Immich's smart search with the text and the structured
    /// filter, in the order of relevance. The caller pages with the `next_page` it is given.
    ///
    /// Immich's structured smart search has no paging (it rejects `page` next to `filter` and
    /// answers without a next page), so page `n` asks for the best `n * 100` results and keeps the
    /// last 100. Immich ranks deterministically (distance, then asset id), so the pages join without
    /// gaps or repeats. At most [`CONTEXT_RESULT_LIMIT`] results can be asked for.
    pub async fn search_context_page(
        &self,
        request: &SearchRequest,
        page: u64,
    ) -> Result<ContextSearchPage, String> {
        let text = request.text.trim();
        if request.mode != SearchMode::Context || text.is_empty() {
            return Err(invalid_request());
        }
        let window = usize::try_from(page)
            .ok()
            .and_then(|page| page.checked_mul(CONTEXT_PAGE_SIZE))
            .filter(|window| (CONTEXT_PAGE_SIZE..=CONTEXT_RESULT_LIMIT).contains(window))
            .ok_or_else(invalid_request)?;
        let body = json!({
            "query": text,
            "filter": build_filter(request),
            "size": window,
            "withExif": true,
        });
        // A 400 means the server rejects smart search although its features reported it on; a
        // server error means its machine-learning service could not be reached.
        let payload: ContextSearchResponse = send_json_mapping_statuses(
            self.request(Method::POST, "search/smart").json(&body),
            MAX_SEARCH_RESPONSE_BYTES,
            StatusCodes {
                bad_request: Some("search_context_unavailable"),
                server_error: Some("search_context_service_unavailable"),
            },
        )
        .await?;
        let filled_the_window = payload.assets.items.len() >= window;
        Ok(ContextSearchPage {
            assets: payload
                .assets
                .items
                .into_iter()
                .skip(window - CONTEXT_PAGE_SIZE)
                .take(CONTEXT_PAGE_SIZE)
                .map(|asset| timeline_asset_from_response(asset, false))
                .collect(),
            next_page: (filled_the_window && window < CONTEXT_RESULT_LIMIT).then_some(page + 1),
            limit_reached: filled_the_window && window == CONTEXT_RESULT_LIMIT,
        })
    }
}

#[cfg(test)]
mod tests;
