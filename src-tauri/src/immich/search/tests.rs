use super::{
    ContextSearchPage, SearchFeatures, SearchFilters, SearchMode, SearchRequest, SearchResults,
    SearchSupport,
};
use crate::immich::search_filters::{CameraFilter, DateRange, LocationFilter, MediaType, Rating};
use crate::immich::test_support::{serve_once, serve_pages, test_client};
use serde_json::{json, Value};
use std::time::Duration;

fn filename_request(text: &str) -> SearchRequest {
    SearchRequest {
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
    request: SearchRequest,
) -> (Result<SearchResults, String>, Vec<String>) {
    let (url, server) = serve_pages(responses).await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        test_client(url, "test-key").search_assets(&request),
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
    let (result, requests) = search(vec![("200 OK", first)], filename_request("IMG")).await;

    let request = &requests[0];
    assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
    assert!(request.contains("x-api-key: test-key"));
    assert_eq!(
        json_body(request),
        json!({
            "filter": {
                "originalFileName": { "like": "%IMG%" },
                "visibility": { "eq": "timeline" },
                "trashedAt": { "eq": null },
            },
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
    let (_, requests) = search(vec![("200 OK", empty)], filename_request(r"100%_a\b")).await;

    assert_eq!(
        json_body(&requests[0])["filter"]["originalFileName"]["like"],
        json!(r"%100\%\_a\\b%")
    );
}

fn mode_request(mode: SearchMode, text: &str) -> SearchRequest {
    SearchRequest {
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
            mode_request(mode, "holiday"),
        )
        .await;

        assert!(result.is_ok(), "{mode:?}");
        let request = &requests[0];
        assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
        assert_eq!(
            without_default_visibility(json_body(request)["filter"].clone()),
            expected,
            "{mode:?}"
        );
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
            mode_request(mode, r"100%_a\b"),
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
        mode_request(SearchMode::Ocr, "100%_a"),
    )
    .await;
    assert_eq!(
        without_default_visibility(json_body(&requests[0])["filter"].clone()),
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
        let (_, requests) = search(
            vec![("200 OK", page(vec![], None))],
            mode_request(mode, " "),
        )
        .await;
        assert_eq!(
            without_default_visibility(json_body(&requests[0])["filter"].clone()),
            json!({}),
            "{mode:?}"
        );
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
        let request: SearchRequest =
            serde_json::from_str(&format!(r#"{{"text":"x","mode":"{name}"}}"#)).unwrap();
        assert_eq!(request.mode, mode);
    }
}

#[tokio::test]
async fn the_text_is_trimmed_and_an_empty_text_sends_no_text_condition() {
    let empty = page(vec![], None);
    let (_, requests) = search(vec![("200 OK", empty)], filename_request("  IMG ")).await;
    assert_eq!(
        json_body(&requests[0])["filter"]["originalFileName"]["like"],
        json!("%IMG%")
    );

    let (_, requests) = search(vec![("200 OK", page(vec![], None))], filename_request("  ")).await;
    assert_eq!(
        without_default_visibility(json_body(&requests[0])["filter"].clone()),
        json!({})
    );
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
        filename_request("x"),
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
    let (result, _) = search(vec![("200 OK", body)], filename_request("x")).await;

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
async fn a_search_stops_at_the_5000_newest_results_and_reports_that_it_was_cut() {
    let pages = (0..5)
        .map(|index| {
            (
                "200 OK",
                full_page(&format!("p{index}"), Some(&format!("c{index}"))),
            )
        })
        .collect();
    let (result, requests) = search(pages, filename_request("x")).await;

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
    let (result, _) = search(pages, filename_request("x")).await;

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
        filename_request("x"),
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
        filename_request("x"),
    )
    .await;

    assert_eq!(requests.len(), 2);
    assert_eq!(result.unwrap_err(), "search_invalid_response");
}

#[tokio::test]
async fn a_repeated_cursor_on_the_page_that_reaches_the_limit_is_rejected() {
    let mut pages: Vec<_> = (0..4)
        .map(|index| {
            (
                "200 OK",
                full_page(&format!("p{index}"), Some(&format!("c{index}"))),
            )
        })
        .collect();
    pages.push(("200 OK", full_page("last", Some("c0"))));
    let (result, _) = search(pages, filename_request("x")).await;

    assert_eq!(result.unwrap_err(), "search_invalid_response");
}

#[tokio::test]
async fn an_empty_page_that_promises_another_page_is_rejected() {
    let first = page(
        vec![asset("a", "2025-03-01T10:00:00.000Z")],
        Some("cursor-1"),
    );
    let empty = page(vec![], Some("cursor-2"));
    let (result, requests) = search(
        vec![("200 OK", first), ("200 OK", empty)],
        filename_request("x"),
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
        let (result, _) = search(vec![(status, "secret-key".into())], filename_request("x")).await;
        assert_eq!(result.unwrap_err(), expected, "{status}");
    }
    let (result, _) = search(vec![("200 OK", "{secret".into())], filename_request("x")).await;
    assert_eq!(result.unwrap_err(), "search_invalid_response");
}

#[tokio::test]
async fn an_unreachable_server_is_unavailable() {
    let (url, server) = serve_pages(Vec::<(String, String)>::new()).await;
    server.await.unwrap();
    let result = test_client(url, "test-key")
        .search_assets(&filename_request("x"))
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
        support_for(r#"{"major":3,"minor":2,"patch":0,"prerelease":null,"featureLevel":1}"#).await;
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
fn search_results_serialize_in_camel_case() {
    let timeline = SearchResults {
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
fn a_request_deserializes_from_the_frontend_shape_with_default_filters() {
    let request: SearchRequest =
        serde_json::from_str(r#"{"text":"IMG","mode":"filename"}"#).unwrap();
    assert_eq!(request, filename_request("IMG"));
    let request: SearchRequest =
        serde_json::from_str(r#"{"text":"IMG","mode":"filename","filters":{}}"#).unwrap();
    assert_eq!(request, filename_request("IMG"));
}

#[tokio::test]
async fn a_context_search_without_text_runs_as_a_chronological_filters_only_search() {
    let (result, requests) = search(
        vec![(
            "200 OK",
            page(vec![asset("a", "2025-03-01T10:00:00.000Z")], None),
        )],
        mode_request(SearchMode::Context, "  "),
    )
    .await;

    assert!(result.is_ok());
    let request = &requests[0];
    assert!(request.starts_with("POST /api/search/metadata HTTP/1.1"));
    assert_eq!(
        json_body(request)["filter"],
        json!({ "visibility": { "eq": "timeline" }, "trashedAt": { "eq": null } })
    );
    assert!(json_body(request).get("query").is_none());
}

#[tokio::test]
async fn a_context_text_is_never_sent_to_the_chronological_search() {
    let (result, requests) = search(Vec::new(), mode_request(SearchMode::Context, "dog")).await;

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
    request: SearchRequest,
    page: u64,
) -> (Result<ContextSearchPage, String>, Vec<String>) {
    let (url, server) = serve_pages(vec![(status, body)]).await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        test_client(url, "test-key").search_context_page(&request, page),
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
        None,
    );
    let (result, requests) = context_page(
        "200 OK",
        body,
        mode_request(SearchMode::Context, " dog on a beach "),
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
            // Archived Assets are left out by default, as in every Search.
            "filter": { "visibility": { "eq": "timeline" }, "trashedAt": { "eq": null } },
            // Structured smart search has no paging (Immich 3.4.1 rejects `page` next to `filter`).
            "size": 100,
            "withExif": true,
        })
    );
    let page = result.unwrap();
    // The server's relevance order is kept and nothing needs a capture time.
    let ids: Vec<_> = page.assets.iter().map(|asset| asset.id.as_str()).collect();
    assert_eq!(ids, vec!["best", "second", "undated"]);
    // Fewer results than asked for: nothing more to load.
    assert_eq!(page.next_page, None);
}

#[tokio::test]
async fn a_context_search_sends_every_search_filter_in_the_structured_filter() {
    let request = SearchRequest {
        filters: SearchFilters {
            archived: true,
            favorite: true,
            ..SearchFilters::default()
        },
        ..mode_request(SearchMode::Context, "dog")
    };
    let (result, requests) = context_page("200 OK", smart_page(vec![], None), request, 1).await;

    assert!(result.is_ok());
    assert_eq!(
        json_body(&requests[0])["filter"],
        json!({ "visibility": { "eq": "archive" }, "trashedAt": { "eq": null }, "isFavorite": { "eq": true } })
    );
}

/// `count` results with ids `r0`, `r1`, ... in the order Immich ranks them.
fn ranked(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| asset(&format!("r{index}"), "2025-03-01T10:00:00.000Z"))
        .collect()
}

fn ids(page: &ContextSearchPage) -> Vec<&str> {
    page.assets.iter().map(|asset| asset.id.as_str()).collect()
}

#[tokio::test]
async fn a_later_context_page_asks_for_a_longer_window_and_keeps_only_its_own_results() {
    // Immich's structured smart search cannot be paged, so page 2 asks for the best 200 and
    // shows results 101 to 200.
    let (result, requests) = context_page(
        "200 OK",
        smart_page(ranked(200), None),
        mode_request(SearchMode::Context, "dog"),
        2,
    )
    .await;

    let body = json_body(&requests[0]);
    assert_eq!(body["size"], json!(200));
    assert!(body.get("page").is_none());
    let page = result.unwrap();
    assert_eq!(page.assets.len(), 100);
    assert_eq!(ids(&page).first(), Some(&"r100"));
    assert_eq!(ids(&page).last(), Some(&"r199"));
    assert_eq!(page.next_page, Some(3));
}

#[tokio::test]
async fn another_context_page_follows_only_while_immich_filled_the_window() {
    let (result, _) = context_page(
        "200 OK",
        smart_page(ranked(100), None),
        mode_request(SearchMode::Context, "dog"),
        1,
    )
    .await;
    assert_eq!(
        result.unwrap().next_page,
        Some(2),
        "a full window may have more"
    );

    let (result, _) = context_page(
        "200 OK",
        smart_page(ranked(150), None),
        mode_request(SearchMode::Context, "dog"),
        2,
    )
    .await;
    let page = result.unwrap();
    assert_eq!(page.assets.len(), 50);
    assert_eq!(page.next_page, None, "an unfilled window is the last page");

    let (result, _) = context_page(
        "200 OK",
        smart_page(vec![], None),
        mode_request(SearchMode::Context, "dog"),
        2,
    )
    .await;
    let page = result.unwrap();
    assert!(page.assets.is_empty());
    assert_eq!(page.next_page, None);
}

#[tokio::test]
async fn a_context_search_shows_at_most_the_best_thousand_results() {
    // 1 000 is the most Immich answers in one request, so page 10 is the last one.
    let (result, requests) = context_page(
        "200 OK",
        smart_page(ranked(1_000), None),
        mode_request(SearchMode::Context, "dog"),
        10,
    )
    .await;
    assert_eq!(json_body(&requests[0])["size"], json!(1_000));
    let page = result.unwrap();
    assert_eq!(page.assets.len(), 100);
    assert_eq!(page.next_page, None);
    // Immich filled the window at the limit, so there may be more than is shown.
    assert!(page.limit_reached);

    let client = test_client("http://127.0.0.1:9".to_string(), "test-key");
    let result = client
        .search_context_page(&mode_request(SearchMode::Context, "dog"), 11)
        .await;
    assert_eq!(result.unwrap_err(), "search_invalid_request");
    let result = client
        .search_context_page(&mode_request(SearchMode::Context, "dog"), 0)
        .await;
    assert_eq!(result.unwrap_err(), "search_invalid_request");
}

#[tokio::test]
async fn the_result_limit_is_reached_only_when_the_last_window_is_full() {
    // Fewer than 1 000 results: the Search ended on its own, nothing was cut.
    let (result, _) = context_page(
        "200 OK",
        smart_page(ranked(950), None),
        mode_request(SearchMode::Context, "dog"),
        10,
    )
    .await;
    let page = result.unwrap();
    assert_eq!(page.next_page, None);
    assert!(!page.limit_reached);

    // A full window before the limit continues with the next page instead.
    let (result, _) = context_page(
        "200 OK",
        smart_page(ranked(100), None),
        mode_request(SearchMode::Context, "dog"),
        1,
    )
    .await;
    let page = result.unwrap();
    assert_eq!(page.next_page, Some(2));
    assert!(!page.limit_reached);
}

#[tokio::test]
async fn a_page_announced_by_the_server_is_not_followed() {
    let (result, _) = context_page(
        "200 OK",
        smart_page(ranked(3), Some("9")),
        mode_request(SearchMode::Context, "dog"),
        1,
    )
    .await;

    assert_eq!(result.unwrap().next_page, None);
}

#[tokio::test]
async fn every_search_leaves_out_trashed_remote_assets() {
    // Immich's structured filter includes trashed Remote Assets unless told otherwise.
    let trashed_left_out = json!({ "eq": null });
    for mode in [SearchMode::Filename, SearchMode::Ocr, SearchMode::Context] {
        let (_, requests) = search(
            vec![("200 OK", page(vec![], None))],
            mode_request(mode, " "),
        )
        .await;
        assert_eq!(
            json_body(&requests[0])["filter"]["trashedAt"],
            trashed_left_out,
            "{mode:?} without text"
        );
    }
    let (_, requests) = search(
        vec![("200 OK", page(vec![], None))],
        mode_request(SearchMode::Filename, "IMG"),
    )
    .await;
    assert_eq!(
        json_body(&requests[0])["filter"]["trashedAt"],
        trashed_left_out
    );
    let (_, requests) = context_page(
        "200 OK",
        smart_page(vec![], None),
        mode_request(SearchMode::Context, "dog"),
        1,
    )
    .await;
    assert_eq!(
        json_body(&requests[0])["filter"]["trashedAt"],
        trashed_left_out
    );
}

#[tokio::test]
async fn a_context_search_needs_a_text_and_sends_nothing_without_one() {
    // Nothing listens here: a request would fail as unavailable, not as an invalid request.
    let client = test_client("http://127.0.0.1:9".to_string(), "test-key");
    for text in ["", "  "] {
        let result = client
            .search_context_page(&mode_request(SearchMode::Context, text), 1)
            .await;
        assert_eq!(result.unwrap_err(), "search_invalid_request");
    }
    let result = client
        .search_context_page(&mode_request(SearchMode::Filename, "dog"), 1)
        .await;
    assert_eq!(result.unwrap_err(), "search_invalid_request");
}

#[tokio::test]
async fn context_search_failures_become_credential_free_codes_without_the_response_body() {
    let cases = [
        ("401 Unauthorized", "search_authentication_required"),
        ("403 Forbidden", "search_permission_denied:asset.read"),
        // Immich answers a server error when its machine-learning service cannot be reached.
        (
            "500 Internal Server Error",
            "search_context_service_unavailable",
        ),
        (
            "503 Service Unavailable",
            "search_context_service_unavailable",
        ),
        // The server rejects smart search although the feature was reported on.
        ("400 Bad Request", "search_context_unavailable"),
    ];
    for (status, expected) in cases {
        let (result, _) = context_page(
            status,
            "secret-key".into(),
            mode_request(SearchMode::Context, "dog"),
            1,
        )
        .await;
        assert_eq!(result.unwrap_err(), expected, "{status}");
    }
    let (result, _) = context_page(
        "200 OK",
        "{secret".into(),
        mode_request(SearchMode::Context, "dog"),
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
        .search_context_page(&mode_request(SearchMode::Context, "dog"), 1)
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
        mode_request(SearchMode::Context, "dog"),
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
        limit_reached: false,
    };
    assert_eq!(
        serde_json::to_string(&page).unwrap(),
        r#"{"assets":[],"nextPage":2,"limitReached":false}"#
    );
}

/// The `filter` object a Search with these Search Filters (and no text) sends.
async fn filter_sent_for(filters: SearchFilters) -> Value {
    let request = SearchRequest {
        filters,
        ..filename_request("")
    };
    let (_, requests) = search(vec![("200 OK", page(vec![], None))], request).await;
    without_default_visibility(json_body(&requests[0])["filter"].clone())
}

/// The `filter` a Search sent, without the default conditions every Search carries (the timeline
/// visibility and the exclusion of trashed Remote Assets), so a test about another condition does
/// not repeat them. Both are asserted on their own.
fn without_default_visibility(mut filter: Value) -> Value {
    let conditions = filter.as_object_mut().unwrap();
    if conditions.get("visibility") == Some(&json!({ "eq": "timeline" })) {
        conditions.remove("visibility");
    }
    conditions.remove("trashedAt");
    filter
}

fn day_range(from: Option<&str>, to: Option<&str>) -> DateRange {
    DateRange {
        from: from.map(str::to_string),
        to: to.map(str::to_string),
    }
}

#[tokio::test]
async fn a_date_range_becomes_inclusive_bounds_on_the_capture_time() {
    let filters = SearchFilters {
        date: Some(day_range(
            Some("2025-02-28T23:00:00.000Z"),
            Some("2025-03-31T21:59:59.999Z"),
        )),
        ..SearchFilters::default()
    };

    assert_eq!(
        filter_sent_for(filters).await,
        json!({ "takenAt": {
            "gte": "2025-02-28T23:00:00.000Z",
            "lte": "2025-03-31T21:59:59.999Z",
        } })
    );
}

#[tokio::test]
async fn either_bound_of_the_date_range_is_optional() {
    let from_only = SearchFilters {
        date: Some(day_range(Some("2025-02-28T23:00:00.000Z"), None)),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(from_only).await,
        json!({ "takenAt": { "gte": "2025-02-28T23:00:00.000Z" } })
    );
    let to_only = SearchFilters {
        date: Some(day_range(None, Some("2025-03-31T21:59:59.999Z"))),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(to_only).await,
        json!({ "takenAt": { "lte": "2025-03-31T21:59:59.999Z" } })
    );
    let neither = SearchFilters {
        date: Some(day_range(None, None)),
        ..SearchFilters::default()
    };
    assert_eq!(filter_sent_for(neither).await, json!({}));
}

#[tokio::test]
async fn the_media_type_selects_images_or_videos() {
    for (media_type, expected) in [(MediaType::Image, "IMAGE"), (MediaType::Video, "VIDEO")] {
        let filters = SearchFilters {
            media_type: Some(media_type),
            ..SearchFilters::default()
        };
        assert_eq!(
            filter_sent_for(filters).await,
            json!({ "type": { "eq": expected } })
        );
    }
}

#[tokio::test]
async fn archived_assets_are_excluded_unless_the_archived_option_is_on() {
    let off = SearchFilters::default();
    let (_, requests) = search(
        vec![("200 OK", page(vec![], None))],
        SearchRequest {
            filters: off,
            ..filename_request("")
        },
    )
    .await;
    assert_eq!(
        json_body(&requests[0])["filter"],
        json!({ "visibility": { "eq": "timeline" }, "trashedAt": { "eq": null } }),
        "a Search without the option asks for the timeline only, whatever Immich's default is"
    );

    let on = SearchFilters {
        archived: true,
        ..SearchFilters::default()
    };
    let (_, requests) = search(
        vec![("200 OK", page(vec![], None))],
        SearchRequest {
            filters: on,
            ..filename_request("")
        },
    )
    .await;
    assert_eq!(
        json_body(&requests[0])["filter"],
        json!({ "visibility": { "eq": "archive" }, "trashedAt": { "eq": null } }),
        "with the option on only Archived Assets are asked for"
    );
}

#[tokio::test]
async fn favorite_restricts_the_search_to_favorites() {
    let filters = SearchFilters {
        favorite: true,
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(filters).await,
        json!({ "isFavorite": { "eq": true } })
    );
}

#[tokio::test]
async fn unfiled_restricts_the_search_to_remote_assets_in_no_album() {
    let filters = SearchFilters {
        unfiled: true,
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(filters).await,
        json!({ "hasAlbums": { "eq": false } })
    );
}

#[tokio::test]
async fn rating_selects_exactly_one_star_level() {
    for stars in 1..=5u8 {
        let filters = SearchFilters {
            rating: Some(Rating::try_from(stars).unwrap()),
            ..SearchFilters::default()
        };
        assert_eq!(
            filter_sent_for(filters).await,
            json!({ "rating": { "eq": stars } })
        );
    }
}

#[tokio::test]
async fn display_options_and_rating_combine_with_each_other_and_with_the_text() {
    let request = SearchRequest {
        filters: SearchFilters {
            favorite: true,
            archived: true,
            unfiled: true,
            rating: Some(Rating::try_from(4).unwrap()),
            ..SearchFilters::default()
        },
        ..filename_request("IMG")
    };
    let (_, requests) = search(vec![("200 OK", page(vec![], None))], request).await;
    assert_eq!(
        json_body(&requests[0])["filter"],
        json!({
            "originalFileName": { "like": "%IMG%" },
            "isFavorite": { "eq": true },
            "visibility": { "eq": "archive" },
            "trashedAt": { "eq": null },
            "hasAlbums": { "eq": false },
            "rating": { "eq": 4 },
        })
    );
}

#[test]
fn a_rating_outside_one_to_five_stars_is_rejected_before_it_reaches_immich() {
    for stars in [0, 6, 255] {
        let json = format!(r#"{{"text":"","mode":"filename","filters":{{"rating":{stars}}}}}"#);
        assert!(
            serde_json::from_str::<SearchRequest>(&json).is_err(),
            "{stars}"
        );
    }
}

#[test]
fn display_options_and_rating_deserialize_from_the_frontend_shape() {
    let request: SearchRequest = serde_json::from_str(
        r#"{"text":"","mode":"filename","filters":{"favorite":true,"archived":true,"unfiled":true,"rating":3}}"#,
    )
    .unwrap();
    assert_eq!(
        request.filters,
        SearchFilters {
            favorite: true,
            archived: true,
            unfiled: true,
            rating: Some(Rating::try_from(3).unwrap()),
            ..SearchFilters::default()
        }
    );
}

#[tokio::test]
async fn chosen_people_must_all_appear_in_the_remote_asset() {
    let filters = SearchFilters {
        person_ids: Some(vec!["person-1".to_string(), "person-2".to_string()]),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(filters).await,
        json!({ "personIds": { "all": ["person-1", "person-2"] } })
    );
}

#[tokio::test]
async fn no_chosen_person_adds_no_person_condition() {
    for person_ids in [None, Some(vec![])] {
        let filters = SearchFilters {
            person_ids,
            ..SearchFilters::default()
        };
        assert_eq!(filter_sent_for(filters).await, json!({}));
    }
}

#[tokio::test]
async fn text_and_every_set_filter_combine_in_one_structured_filter() {
    let request = SearchRequest {
        filters: SearchFilters {
            date: Some(day_range(Some("2025-02-28T23:00:00.000Z"), None)),
            media_type: Some(MediaType::Video),
            person_ids: Some(vec!["person-1".to_string()]),
            ..SearchFilters::default()
        },
        ..filename_request("IMG")
    };
    let (_, requests) = search(vec![("200 OK", page(vec![], None))], request).await;

    let body = json_body(&requests[0]);
    assert_eq!(
        body["filter"],
        json!({
            "originalFileName": { "like": "%IMG%" },
            "takenAt": { "gte": "2025-02-28T23:00:00.000Z" },
            "type": { "eq": "VIDEO" },
            "personIds": { "all": ["person-1"] },
            "visibility": { "eq": "timeline" },
            "trashedAt": { "eq": null },
        })
    );
    assert_eq!(body.as_object().unwrap().len(), 4, "no flat filter fields");
}

#[test]
fn filters_deserialize_from_the_frontend_shape() {
    let request: SearchRequest = serde_json::from_str(
        r#"{"text":"","mode":"filename","filters":{"date":{"from":"2025-01-01T00:00:00.000Z"},"mediaType":"image","personIds":["person-1"]}}"#,
    )
    .unwrap();
    assert_eq!(
        request.filters,
        SearchFilters {
            date: Some(day_range(Some("2025-01-01T00:00:00.000Z"), None)),
            media_type: Some(MediaType::Image),
            person_ids: Some(vec!["person-1".to_string()]),
            ..SearchFilters::default()
        }
    );
}

fn camera(make: Option<&str>, model: Option<&str>, lens_model: Option<&str>) -> CameraFilter {
    CameraFilter {
        make: make.map(str::to_string),
        model: model.map(str::to_string),
        lens_model: lens_model.map(str::to_string),
    }
}

#[tokio::test]
async fn the_camera_make_model_and_lens_become_exact_conditions() {
    let filters = SearchFilters {
        camera: Some(camera(Some("FUJIFILM"), Some("X100V"), Some("23mm F2"))),
        ..SearchFilters::default()
    };

    assert_eq!(
        filter_sent_for(filters).await,
        json!({
            "make": { "eq": "FUJIFILM" },
            "model": { "eq": "X100V" },
            "lensModel": { "eq": "23mm F2" },
        })
    );
}

#[tokio::test]
async fn each_camera_part_is_optional_and_a_blank_part_is_no_condition() {
    let make_only = SearchFilters {
        camera: Some(camera(Some("Canon"), None, None)),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(make_only).await,
        json!({ "make": { "eq": "Canon" } })
    );
    let lens_only = SearchFilters {
        camera: Some(camera(None, None, Some("RF 50mm"))),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(lens_only).await,
        json!({ "lensModel": { "eq": "RF 50mm" } })
    );
    let blank = SearchFilters {
        camera: Some(camera(Some(" "), Some(""), None)),
        ..SearchFilters::default()
    };
    assert_eq!(filter_sent_for(blank).await, json!({}));
}

#[test]
fn the_camera_filter_deserializes_from_the_frontend_shape() {
    let request: SearchRequest = serde_json::from_str(
        r#"{"text":"","mode":"filename","filters":{"camera":{"make":"Canon","lensModel":"RF 50mm"}}}"#,
    )
    .unwrap();
    assert_eq!(
        request.filters.camera,
        Some(camera(Some("Canon"), None, Some("RF 50mm")))
    );
}

fn location(country: Option<&str>, state: Option<&str>) -> LocationFilter {
    LocationFilter {
        country: country.map(str::to_string),
        state: state.map(str::to_string),
        city: None,
    }
}

fn city_location(country: Option<&str>, state: Option<&str>, city: Option<&str>) -> LocationFilter {
    LocationFilter {
        city: city.map(str::to_string),
        ..location(country, state)
    }
}

#[tokio::test]
async fn the_location_country_and_state_become_exact_conditions() {
    let filters = SearchFilters {
        location: Some(location(Some("Italy"), Some("Tuscany"))),
        ..SearchFilters::default()
    };

    assert_eq!(
        filter_sent_for(filters).await,
        json!({
            "country": { "eq": "Italy" },
            "state": { "eq": "Tuscany" },
        })
    );
}

#[tokio::test]
async fn each_location_part_is_optional_and_a_blank_part_is_no_condition() {
    let country_only = SearchFilters {
        location: Some(location(Some("Italy"), None)),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(country_only).await,
        json!({ "country": { "eq": "Italy" } })
    );
    let state_only = SearchFilters {
        location: Some(location(None, Some("Bavaria"))),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(state_only).await,
        json!({ "state": { "eq": "Bavaria" } })
    );
    let blank = SearchFilters {
        location: Some(location(Some(" "), Some(""))),
        ..SearchFilters::default()
    };
    assert_eq!(filter_sent_for(blank).await, json!({}));
}

#[tokio::test]
async fn the_location_combines_with_every_other_filter() {
    let filters = SearchFilters {
        date: Some(day_range(Some("2025-01-01T00:00:00.000Z"), None)),
        media_type: Some(MediaType::Image),
        camera: Some(camera(Some("Canon"), None, None)),
        location: Some(location(Some("Italy"), None)),
        favorite: true,
        ..SearchFilters::default()
    };

    assert_eq!(
        filter_sent_for(filters).await,
        json!({
            "isFavorite": { "eq": true },
            "takenAt": { "gte": "2025-01-01T00:00:00.000Z" },
            "type": { "eq": "IMAGE" },
            "make": { "eq": "Canon" },
            "country": { "eq": "Italy" },
        })
    );
}

#[test]
fn the_location_filter_deserializes_from_the_frontend_shape() {
    let request: SearchRequest = serde_json::from_str(
        r#"{"text":"","mode":"filename","filters":{"location":{"country":"Italy"}}}"#,
    )
    .unwrap();
    assert_eq!(
        request.filters.location,
        Some(location(Some("Italy"), None))
    );
}

#[tokio::test]
async fn the_city_becomes_an_exact_condition_with_the_name_exactly_as_immich_returned_it() {
    let filters = SearchFilters {
        location: Some(city_location(None, None, Some(" Saint-Étienne "))),
        ..SearchFilters::default()
    };

    assert_eq!(
        filter_sent_for(filters).await,
        json!({ "city": { "eq": " Saint-Étienne " } })
    );
}

#[tokio::test]
async fn the_city_combines_with_the_country_and_state_and_a_blank_city_is_no_condition() {
    let all_three = SearchFilters {
        location: Some(city_location(
            Some("Italy"),
            Some("Tuscany"),
            Some("Florence"),
        )),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(all_three).await,
        json!({
            "country": { "eq": "Italy" },
            "state": { "eq": "Tuscany" },
            "city": { "eq": "Florence" },
        })
    );
    let blank = SearchFilters {
        location: Some(city_location(Some("Italy"), None, Some(" "))),
        ..SearchFilters::default()
    };
    assert_eq!(
        filter_sent_for(blank).await,
        json!({ "country": { "eq": "Italy" } })
    );
}

#[test]
fn the_location_filter_deserializes_a_city_from_the_frontend_shape() {
    let request: SearchRequest = serde_json::from_str(
        r#"{"text":"","mode":"filename","filters":{"location":{"city":"Florence"}}}"#,
    )
    .unwrap();
    assert_eq!(
        request.filters.location,
        Some(city_location(None, None, Some("Florence")))
    );
}
