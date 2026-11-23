//! Seam 1: the public interface of the projection, driven against a fake Immich server.

use super::*;
use crate::immich::test_support::{serve_routes, test_client, RecordedRequest, RoutedServer};
use crate::immich::RemoteFailureKind;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// One Remote Asset of the fake Library.
#[derive(Clone)]
struct FakeAsset {
    id: &'static str,
    name: &'static str,
    local_date_time: Option<&'static str>,
    file_created_at: &'static str,
    size: u64,
    visibility: &'static str,
    trashed: bool,
    /// Immich's `livePhotoVideoId`: the hidden video asset paired with this still.
    live_video: Option<&'static str>,
}

/// An asset captured at `local_date_time` in UTC, so its local and UTC capture time agree. One
/// without a usable capture time is created at a fixed instant.
fn asset(id: &'static str, name: &'static str, local_date_time: &'static str) -> FakeAsset {
    FakeAsset {
        id,
        name,
        local_date_time: Some(local_date_time),
        file_created_at: if DateTime::parse_from_rfc3339(local_date_time).is_ok() {
            local_date_time
        } else {
            "2025-01-01T00:00:00.000Z"
        },
        size: 1_000,
        visibility: "timeline",
        trashed: false,
        live_video: None,
    }
}

/// A Live Photo's video half: a separate asset Immich hides from the timeline.
fn live_video(id: &'static str, name: &'static str, size: u64) -> FakeAsset {
    FakeAsset {
        size,
        visibility: "hidden",
        ..asset(id, name, "2025-03-10T10:00:00.000Z")
    }
}

impl FakeAsset {
    fn json(&self) -> Value {
        let mut item = json!({
            "id": self.id,
            "originalFileName": self.name,
            "fileCreatedAt": self.file_created_at,
            "fileModifiedAt": self.file_created_at,
            "visibility": self.visibility,
            "isTrashed": self.trashed,
            "type": "IMAGE",
            "exifInfo": { "fileSizeInByte": self.size },
        });
        if let Some(local) = self.local_date_time {
            item["localDateTime"] = local.into();
        }
        if let Some(video) = self.live_video {
            item["livePhotoVideoId"] = video.into();
        }
        item
    }
}

/// One Album of the fake Library.
#[derive(Clone)]
struct FakeAlbum {
    id: &'static str,
    name: &'static str,
    /// Whether the user owns it; otherwise it is shared with the user.
    owned: bool,
    /// Whether it is shared with other users (an owned Album can be).
    shared: bool,
    assets: Vec<&'static str>,
}

fn album(id: &'static str, name: &'static str, assets: &[&'static str]) -> FakeAlbum {
    FakeAlbum {
        id,
        name,
        owned: true,
        shared: false,
        assets: assets.to_vec(),
    }
}

impl FakeAlbum {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "albumName": self.name,
            "assetCount": self.assets.len(),
            "shared": self.shared,
            "albumThumbnailAssetId": null,
        })
    }
}

/// An Immich server that holds `assets` and answers metadata searches the way Immich does:
/// without a `visibility` filter every asset outside the locked folder matches (Archived
/// ones too), Trashed Assets only match with `withDeleted`.
async fn fake_library(assets: Vec<FakeAsset>) -> RoutedServer {
    fake_immich(assets, vec![]).await
}

/// [`fake_library`] with Albums. `GET /albums` answers the way Immich does: only the Albums the
/// user owns, and with `shared=true` the Albums shared with the user or by the user. A metadata
/// search with `albumIds` matches the members of those Albums.
async fn fake_immich(assets: Vec<FakeAsset>, albums: Vec<FakeAlbum>) -> RoutedServer {
    fake_immich_shared(Arc::new(Mutex::new(assets)), albums, Arc::default()).await
}

/// The month Immich files `asset` under in its timeline buckets: the components of its local
/// capture time. Immich has no capture time to bucket an asset without a usable one by, so the
/// fake reports those under a bucket that names no calendar month.
fn bucket_name(asset: &FakeAsset) -> String {
    match asset.local_date_time {
        Some(local) if local.len() >= 7 && local.as_bytes()[4] == b'-' => {
            match (local[..4].parse::<u32>(), local[5..7].parse::<u32>()) {
                (Ok(year), Ok(month)) => format!("{year:04}-{month:02}-01T00:00:00.000Z"),
                _ => "undated".to_string(),
            }
        }
        _ => "undated".to_string(),
    }
}

/// [`fake_immich`] over a Library the test can change while the server runs, which also counts
/// how many assets the server sent in metadata search pages (`served`). Besides the metadata
/// search it answers `GET /timeline/buckets` (months with their asset counts) the way Immich
/// does: by local capture month, for the assets of the requested visibility.
async fn fake_immich_shared(
    library: Arc<Mutex<Vec<FakeAsset>>>,
    albums: Vec<FakeAlbum>,
    served: Arc<AtomicUsize>,
) -> RoutedServer {
    serve_routes(move |request: &RecordedRequest| {
        let assets = library.lock().unwrap().clone();
        if let Some(id) = request.target.strip_prefix("/api/assets/") {
            return match assets.iter().find(|asset| asset.id == id) {
                Some(asset) => ("200 OK".into(), asset.json().to_string()),
                None => ("404 Not Found".into(), "{}".into()),
            };
        }
        if request.method == "GET" && request.target.starts_with("/api/timeline/buckets") {
            let timeline_only = request.target.contains("visibility=timeline");
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for asset in assets.iter().filter(|asset| {
                !asset.trashed
                    && if timeline_only {
                        asset.visibility == "timeline"
                    } else {
                        asset.visibility != "locked"
                    }
            }) {
                *counts.entry(bucket_name(asset)).or_default() += 1;
            }
            let buckets: Vec<Value> = counts
                .into_iter()
                .rev()
                .map(|(time_bucket, count)| json!({ "timeBucket": time_bucket, "count": count }))
                .collect();
            return ("200 OK".into(), Value::Array(buckets).to_string());
        }
        match (request.method.as_str(), request.target.as_str()) {
            ("GET", "/api/albums") => {
                let owned: Vec<Value> = albums
                    .iter()
                    .filter(|album| album.owned)
                    .map(FakeAlbum::json)
                    .collect();
                return ("200 OK".into(), Value::Array(owned).to_string());
            }
            ("GET", "/api/albums?shared=true") => {
                let shared: Vec<Value> = albums
                    .iter()
                    .filter(|album| !album.owned || album.shared)
                    .map(FakeAlbum::json)
                    .collect();
                return ("200 OK".into(), Value::Array(shared).to_string());
            }
            ("POST", "/api/search/metadata") => {}
            _ => return ("404 Not Found".into(), "{}".into()),
        }
        let body: Value = serde_json::from_str(&request.body).unwrap();
        let visibility = body["visibility"].as_str();
        let with_deleted = body["withDeleted"] == json!(true);
        let in_albums = |asset: &FakeAsset| {
            body["albumIds"].as_array().is_none_or(|ids| {
                albums.iter().any(|album| {
                    ids.iter().any(|id| id == album.id) && album.assets.contains(&asset.id)
                })
            })
        };
        // Immich's `takenAfter` and `takenBefore` bound `fileCreatedAt`, which is UTC.
        let bound = |key: &str| {
            body[key].as_str().map(|text| {
                DateTime::parse_from_rfc3339(text)
                    .unwrap()
                    .with_timezone(&Utc)
            })
        };
        let (after, before) = (bound("takenAfter"), bound("takenBefore"));
        let in_window = |asset: &FakeAsset| {
            (after.is_none() && before.is_none())
                || DateTime::parse_from_rfc3339(asset.file_created_at).is_ok_and(|time| {
                    after.is_none_or(|after| time >= after)
                        && before.is_none_or(|before| time <= before)
                })
        };
        let matching: Vec<&FakeAsset> = assets
            .iter()
            .filter(|asset| {
                visibility.map_or(asset.visibility != "locked", |v| asset.visibility == v)
            })
            .filter(|asset| with_deleted || !asset.trashed)
            .filter(|asset| in_albums(asset))
            .filter(|asset| in_window(asset))
            .collect();
        let page = body["page"].as_u64().unwrap_or(1) as usize;
        let size = body["size"].as_u64().unwrap_or(250) as usize;
        let start = ((page - 1) * size).min(matching.len());
        let end = (start + size).min(matching.len());
        let items: Vec<Value> = matching[start..end]
            .iter()
            .map(|asset| asset.json())
            .collect();
        served.fetch_add(items.len(), Ordering::SeqCst);
        let next_page = (end < matching.len()).then(|| (page + 1).to_string());
        let page = json!({
            "assets": { "items": items, "nextPage": next_page, "total": matching.len() }
        });
        ("200 OK".into(), page.to_string())
    })
    .await
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

#[tokio::test]
async fn the_root_offers_all_photos_and_albums_without_asking_immich() {
    let server = fake_library(vec![]).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let root = library.list("").await.unwrap();

    assert_eq!(names(&root), ["All Photos", "Albums"]);
    assert!(root.iter().all(|entry| entry.kind == EntryKind::Directory));
    assert!(root.iter().all(|entry| entry.remote_asset.is_none()));
    assert!(server.requests().is_empty());
}

/// Photos taken within an hour of local midnight, whose UTC day differs from their local day.
fn near_midnight_library() -> Vec<FakeAsset> {
    vec![
        // Taken 23:30 on 31 March at UTC-2: UTC already says 1 April.
        FakeAsset {
            file_created_at: "2025-04-01T01:30:00.000Z",
            ..asset("a1", "IMG_0001.jpg", "2025-03-31T23:30:00.000Z")
        },
        // Taken 00:30 on 1 April at UTC+2: UTC still says 31 March.
        FakeAsset {
            file_created_at: "2025-03-31T22:30:00.000Z",
            ..asset("a2", "IMG_0002.jpg", "2025-04-01T00:30:00.000Z")
        },
        // Taken 23:45 on New Year's Eve at UTC-2: UTC already says the next year.
        FakeAsset {
            file_created_at: "2025-01-01T01:45:00.000Z",
            ..asset("a3", "IMG_0003.jpg", "2024-12-31T23:45:00.000Z")
        },
    ]
}

#[tokio::test]
async fn all_photos_lists_years_and_months_by_local_capture_time() {
    let server = fake_library(near_midnight_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let years = library.list("All Photos").await.unwrap();
    assert_eq!(names(&years), ["2024", "2025"]);
    assert!(years.iter().all(|entry| entry.kind == EntryKind::Directory));

    let months_2025 = library.list("All Photos/2025").await.unwrap();
    assert_eq!(names(&months_2025), ["03 March", "04 April"]);
    let months_2024 = library.list("All Photos/2024").await.unwrap();
    assert_eq!(names(&months_2024), ["12 December"]);
}

#[tokio::test]
async fn a_month_lists_its_assets_as_files_describing_their_remote_asset() {
    let server = fake_library(near_midnight_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();
    assert_eq!(march.len(), 1);
    let file = &march[0];
    assert_eq!(file.name, "IMG_0001.jpg");
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.remote_asset, Some(RemoteAssetId::new("a1")));
    assert_eq!(file.size, Some(1_000));
    assert_eq!(
        file.created.map(|created| created.to_rfc3339()),
        Some("2025-04-01T01:30:00+00:00".to_string())
    );

    let april = library.list("All Photos/2025/04 April").await.unwrap();
    assert_eq!(names(&april), ["IMG_0002.jpg"]);
}

#[tokio::test]
async fn a_directory_without_assets_does_not_exist() {
    let server = fake_library(near_midnight_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    for path in [
        "All Photos/1999",
        "All Photos/2025/05 May",
        "All Photos/2025/3",
        "Nowhere",
    ] {
        assert_eq!(
            library.list(path).await,
            Err(VirtualLibraryError::NotFound),
            "{path}"
        );
    }
    assert_eq!(
        library.list("All Photos/2025/03 March/IMG_0001.jpg").await,
        Err(VirtualLibraryError::NotADirectory)
    );
}

#[tokio::test]
async fn assets_without_a_usable_capture_time_are_filed_under_undated() {
    let server = fake_library(vec![
        asset("d1", "IMG_0100.jpg", "2025-06-15T12:00:00.000Z"),
        FakeAsset {
            local_date_time: None,
            ..asset("u1", "scan.png", "")
        },
        asset("u2", "broken.png", "not a date"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        names(&library.list("All Photos").await.unwrap()),
        ["2025", "Undated"]
    );
    let undated = library.list("All Photos/Undated").await.unwrap();
    assert_eq!(names(&undated), ["broken.png", "scan.png"]);
    assert_eq!(
        library.resolve("All Photos/Undated/scan.png").await,
        Ok(RemoteAssetId::new("u1"))
    );
    assert_eq!(
        names(&library.list("All Photos/2025/06 June").await.unwrap()),
        ["IMG_0100.jpg"]
    );
}

#[tokio::test]
async fn an_asset_whose_capture_time_is_the_unix_epoch_is_filed_under_undated_not_1970() {
    // Immich reports `localDateTime` = 1970-01-01T00:00:00.000Z for an asset without a capture
    // time (observed on a real server: a `.3gp` landed in `All Photos/1970/01 January`).
    let server = fake_library(vec![
        asset("d1", "IMG_0100.jpg", "2025-06-15T12:00:00.000Z"),
        asset("e1", "Garfild.3gp", "1970-01-01T00:00:00.000Z"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        names(&library.list("All Photos").await.unwrap()),
        ["2025", "Undated"]
    );
    assert_eq!(
        names(&library.list("All Photos/Undated").await.unwrap()),
        ["Garfild.3gp"]
    );
    assert_eq!(
        library.list("All Photos/1970").await,
        Err(VirtualLibraryError::NotFound)
    );
    assert_eq!(
        library.resolve("All Photos/Undated/Garfild.3gp").await,
        Ok(RemoteAssetId::new("e1"))
    );
}

#[tokio::test]
async fn a_library_whose_only_asset_has_the_epoch_capture_time_shows_only_undated() {
    let server = fake_library(vec![asset("e1", "Garfild.3gp", "1970-01-01T00:00:00.000Z")]).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        names(&library.list("All Photos").await.unwrap()),
        ["Undated"]
    );
}

/// A server whose timeline says `2016-04` holds `count` assets, while the metadata search finds
/// `found` of them in any window: Immich disagreeing with itself.
async fn disagreeing_server(count: usize, found: Vec<FakeAsset>) -> RoutedServer {
    serve_routes(move |request: &RecordedRequest| {
        if request.method == "GET" && request.target.starts_with("/api/timeline/buckets") {
            let buckets = json!([{ "timeBucket": "2016-04-01T00:00:00.000Z", "count": count }]);
            return ("200 OK".into(), buckets.to_string());
        }
        if request.target == "/api/search/metadata" {
            let body: Value = serde_json::from_str(&request.body).unwrap();
            // Only a search without a window (the full pass) finds them.
            let windowed = body.get("takenAfter").is_some();
            let items: Vec<Value> = if windowed {
                vec![]
            } else {
                found.iter().map(FakeAsset::json).collect()
            };
            let page = json!({ "assets": { "items": items, "nextPage": null, "total": items.len() } });
            return ("200 OK".into(), page.to_string());
        }
        ("404 Not Found".into(), "{}".into())
    })
    .await
}

#[tokio::test]
async fn a_month_the_timeline_reports_is_found_even_when_the_capture_window_misses_its_assets() {
    // Captured in April 2016 by its local time, but Immich holds a creation instant years later, so
    // a window around April 2016 does not contain it.
    let late = FakeAsset {
        file_created_at: "2024-01-05T00:00:00.000Z",
        ..asset("a1", "2016-04-29_182422_000.mp4", "2016-04-29T18:24:22.000Z")
    };
    let server = fake_library(vec![late]).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        names(&library.list("All Photos/2016").await.unwrap()),
        ["04 April"]
    );
    let april = library.list("All Photos/2016/04 April").await;

    assert_eq!(
        names(&april.expect("the month the timeline reports is not empty")),
        ["2016-04-29_182422_000.mp4"]
    );
    assert_eq!(
        library
            .resolve("All Photos/2016/04 April/2016-04-29_182422_000.mp4")
            .await,
        Ok(RemoteAssetId::new("a1"))
    );
}

#[tokio::test]
async fn a_month_that_stays_empty_although_the_timeline_reports_assets_says_why() {
    let server = disagreeing_server(3, vec![]).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        library.list("All Photos/2016/04 April").await,
        Err(VirtualLibraryError::NotFound)
    );
    let why = library
        .explain_missing("All Photos/2016/04 April")
        .await
        .expect("an explanation");

    assert!(why.contains("2016-04"), "{why}");
    assert!(why.contains("3 asset(s)"), "{why}");
    assert!(why.contains("0 asset(s)"), "{why}");
}

#[tokio::test]
async fn a_directory_that_is_genuinely_absent_needs_no_explanation_of_assets() {
    let server = fake_library(near_midnight_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let why = library
        .explain_missing("All Photos/2016/04 April")
        .await
        .expect("an explanation");

    assert!(why.contains("reports no asset"), "{why}");
}

#[tokio::test]
async fn resolving_a_file_returns_the_remote_asset_it_represents() {
    let server = fake_library(near_midnight_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        library
            .resolve("All Photos/2025/04 April/IMG_0002.jpg")
            .await,
        Ok(RemoteAssetId::new("a2"))
    );
    // Windows hands out backslash paths and compares names without regard to case.
    assert_eq!(
        library
            .resolve(r"all photos\2024\12 december\img_0003.JPG")
            .await,
        Ok(RemoteAssetId::new("a3"))
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0002.jpg")
            .await,
        Err(VirtualLibraryError::NotFound)
    );
    assert_eq!(
        library.resolve("All Photos/2025/03 March").await,
        Err(VirtualLibraryError::NotAFile)
    );
    assert_eq!(
        library.resolve("").await,
        Err(VirtualLibraryError::NotAFile)
    );
}

#[tokio::test]
async fn all_photos_follows_the_photo_timeline_and_never_shows_archived_trashed_or_locked_assets() {
    let visible = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        visible("v1", "visible.jpg"),
        FakeAsset {
            visibility: "archive",
            ..visible("x1", "archived.jpg")
        },
        FakeAsset {
            trashed: true,
            ..visible("x2", "trashed.jpg")
        },
        FakeAsset {
            visibility: "locked",
            ..visible("x3", "locked.jpg")
        },
        // The video half of a Live Photo is hidden by Immich.
        FakeAsset {
            visibility: "hidden",
            ..visible("x4", "live.mov")
        },
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(names(&march), ["visible.jpg"]);
    for hidden in ["archived.jpg", "trashed.jpg", "locked.jpg", "live.mov"] {
        assert_eq!(
            library
                .resolve(&format!("All Photos/2025/03 March/{hidden}"))
                .await,
            Err(VirtualLibraryError::NotFound),
            "{hidden}"
        );
    }
}

#[tokio::test]
async fn an_unreachable_or_refusing_server_is_reported_without_credentials() {
    let server = serve_routes(|_: &RecordedRequest| ("401 Unauthorized".into(), "{}".into())).await;
    let client = test_client(server.url.clone(), "secret-api-key");
    let library = VirtualLibrary::new(&client);

    let Err(VirtualLibraryError::Remote(failure)) = library.list("All Photos").await else {
        panic!("expected a remote error");
    };
    assert_eq!(failure.kind, RemoteFailureKind::Unauthorized);
    assert!(!failure.message.contains("secret-api-key"));
    // The root and Albums structure need no server.
    assert!(library.list("").await.is_ok());
}

/// A Live Photo: the still, and the hidden video its `livePhotoVideoId` names.
fn live_photo_library() -> Vec<FakeAsset> {
    vec![
        FakeAsset {
            live_video: Some("v-live"),
            size: 3_000,
            ..asset("s-live", "IMG_0001.HEIC", "2025-03-10T10:00:00.000Z")
        },
        live_video("v-live", "IMG_0001.MOV", 9_000),
        asset("p1", "IMG_0002.jpg", "2025-03-11T10:00:00.000Z"),
    ]
}

#[tokio::test]
async fn a_live_photo_is_listed_as_a_still_and_a_video_sharing_one_base_name() {
    let server = fake_library(live_photo_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        ["IMG_0001.HEIC", "IMG_0001.MOV", "IMG_0002.jpg"]
    );
    let (still, video) = (&march[0], &march[1]);
    assert_eq!(still.kind, EntryKind::File);
    assert_eq!(video.kind, EntryKind::File);
    assert_eq!(still.size, Some(3_000));
    assert_eq!(video.size, Some(9_000));
    assert_eq!(video.created, still.created);
    assert_eq!(video.modified, still.modified);
}

#[tokio::test]
async fn resolving_each_half_of_a_live_photo_returns_its_own_remote_asset() {
    let server = fake_library(live_photo_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0001.HEIC")
            .await,
        Ok(RemoteAssetId::new("s-live"))
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/img_0001.mov")
            .await,
        Ok(RemoteAssetId::new("v-live"))
    );
}

#[tokio::test]
async fn the_video_of_a_live_photo_is_not_also_listed_as_an_asset_of_its_own() {
    let server = fake_library(live_photo_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    let video_entries = march
        .iter()
        .filter(|entry| entry.remote_asset == Some(RemoteAssetId::new("v-live")))
        .count();
    assert_eq!(video_entries, 1);
    // The Immich-side file name of the hidden video does not leak into any other directory.
    assert_eq!(
        names(&library.list("All Photos/2025").await.unwrap()),
        ["03 March"]
    );
}

#[tokio::test]
async fn the_video_takes_the_base_name_of_the_still_and_the_extension_of_its_own_file() {
    let server = fake_library(vec![
        // An Android motion photo: the video's own name has another base and extension.
        FakeAsset {
            live_video: Some("v-motion"),
            ..asset("s-motion", "PXL_20250310.jpg", "2025-03-10T10:00:00.000Z")
        },
        live_video("v-motion", "MVIMG_20250310.mp4", 7_000),
        // A Live Photo of an asset that has no extension at all, and no capture time.
        FakeAsset {
            live_video: Some("v-bare"),
            local_date_time: None,
            ..asset("s-bare", "scan", "")
        },
        live_video("v-bare", "clip", 5_000),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();
    assert_eq!(names(&march), ["PXL_20250310.jpg", "PXL_20250310.mp4"]);
    let undated = library.list("All Photos/Undated").await.unwrap();
    assert_eq!(names(&undated), ["scan", "scan.mov"]);
    assert_eq!(
        library.resolve("All Photos/Undated/scan.mov").await,
        Ok(RemoteAssetId::new("v-bare"))
    );
}

#[tokio::test]
async fn a_still_whose_video_cannot_be_found_is_listed_alone() {
    let server = fake_library(vec![
        FakeAsset {
            live_video: Some("gone"),
            ..asset("s1", "IMG_0001.HEIC", "2025-03-10T10:00:00.000Z")
        },
        asset("p1", "IMG_0002.jpg", "2025-03-11T10:00:00.000Z"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(names(&march), ["IMG_0001.HEIC", "IMG_0002.jpg"]);
}

#[tokio::test]
async fn directories_that_hold_no_files_do_not_look_up_live_photo_videos() {
    let server = fake_library(live_photo_library()).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    library.list("All Photos").await.unwrap();
    library.list("All Photos/2025").await.unwrap();

    assert!(server
        .requests()
        .iter()
        .all(|request| !request.target.starts_with("/api/assets/")));
}

/// The step Album folders call on their asset list: Immich lists an Album's hidden members, so
/// the video must be dropped from the list when its still is there, and kept as a plain file
/// when it is not (a video added to the Album without its still is an ordinary asset).
mod album_style_lists {
    use super::*;
    use crate::immich::LibraryAsset;

    fn library_asset(id: &str, name: &str, video: Option<&str>) -> LibraryAsset {
        LibraryAsset {
            id: RemoteAssetId::new(id),
            original_file_name: Some(name.to_string()),
            local_date_time: None,
            file_created_at: None,
            file_modified_at: None,
            file_size: None,
            live_photo_video_id: video.map(RemoteAssetId::new),
        }
    }

    #[test]
    fn a_paired_video_among_the_assets_is_not_a_standalone_asset() {
        let assets = vec![
            library_asset("still", "IMG_0001.HEIC", Some("video")),
            library_asset("video", "IMG_0001.MOV", None),
            library_asset("other", "IMG_0002.jpg", None),
        ];

        let kept = live_photo::without_paired_videos(assets);

        let ids: Vec<&str> = kept.iter().map(|asset| asset.id.as_str()).collect();
        assert_eq!(ids, ["still", "other"]);
    }

    #[test]
    fn an_unpaired_video_stays_an_asset() {
        let assets = vec![library_asset("video", "clip.mov", None)];

        assert_eq!(live_photo::without_paired_videos(assets).len(), 1);
    }
}

fn album_library() -> (Vec<FakeAsset>, Vec<FakeAlbum>) {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let assets = vec![
        in_march("p1", "beach.jpg"),
        in_march("p2", "sunset.jpg"),
        FakeAsset {
            visibility: "archive",
            ..in_march("p3", "archived.jpg")
        },
        FakeAsset {
            trashed: true,
            ..in_march("p4", "trashed.jpg")
        },
        in_march("p5", "unfiled.jpg"),
    ];
    let albums = vec![album("al1", "Holiday", &["p1", "p2", "p3", "p4"])];
    (assets, albums)
}

#[tokio::test]
async fn albums_lists_owned_and_shared_albums_once_each() {
    let (assets, mut albums) = album_library();
    albums.extend([
        // Owned and shared out: Immich lists it both with and without `shared=true`.
        FakeAlbum {
            shared: true,
            ..album("al2", "Family", &[])
        },
        // Owned by someone else and shared with the user: only `shared=true` lists it.
        FakeAlbum {
            owned: false,
            shared: true,
            ..album("al3", "From Anna", &[])
        },
    ]);
    let server = fake_immich(assets, albums).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let listed = library.list("Albums").await.unwrap();

    assert_eq!(names(&listed), ["Family", "From Anna", "Holiday"]);
    assert!(listed
        .iter()
        .all(|entry| entry.kind == EntryKind::Directory));
    assert!(listed.iter().all(|entry| entry.remote_asset.is_none()));
}

#[tokio::test]
async fn an_album_folder_holds_exactly_the_assets_immich_returns_for_it() {
    let (assets, albums) = album_library();
    let server = fake_immich(assets, albums).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let holiday = library.list("Albums/Holiday").await.unwrap();

    // Archived Assets are included, Trashed ones never, and an Asset of no Album stays out.
    assert_eq!(names(&holiday), ["archived.jpg", "beach.jpg", "sunset.jpg"]);
    assert!(holiday.iter().all(|entry| entry.kind == EntryKind::File));
    assert_eq!(holiday[1].remote_asset, Some(RemoteAssetId::new("p1")));
    assert_eq!(holiday[1].size, Some(1_000));
    assert_eq!(
        library.resolve("Albums/Holiday/archived.jpg").await,
        Ok(RemoteAssetId::new("p3"))
    );
    for absent in ["trashed.jpg", "unfiled.jpg"] {
        assert_eq!(
            library.resolve(&format!("Albums/Holiday/{absent}")).await,
            Err(VirtualLibraryError::NotFound),
            "{absent}"
        );
    }
    // All Photos keeps following the Photo Timeline: the Archived Asset is not mixed in.
    let march = library.list("All Photos/2025/03 March").await.unwrap();
    assert_eq!(names(&march), ["beach.jpg", "sunset.jpg", "unfiled.jpg"]);
}

#[tokio::test]
async fn shared_albums_open_like_owned_ones() {
    let (assets, mut albums) = album_library();
    albums.push(FakeAlbum {
        owned: false,
        shared: true,
        ..album("al3", "From Anna", &["p5"])
    });
    let server = fake_immich(assets, albums).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(
        names(&library.list("Albums/From Anna").await.unwrap()),
        ["unfiled.jpg"]
    );
    // Windows hands out backslash paths and compares names without regard to case.
    assert_eq!(
        library.resolve(r"albums\from anna\UNFILED.jpg").await,
        Ok(RemoteAssetId::new("p5"))
    );
}

#[tokio::test]
async fn an_empty_album_is_an_empty_folder_and_an_unknown_one_does_not_exist() {
    let server = fake_immich(vec![], vec![album("al9", "Empty", &[])]).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    assert_eq!(library.list("Albums/Empty").await, Ok(vec![]));
    assert_eq!(
        library.list("Albums/Missing").await,
        Err(VirtualLibraryError::NotFound)
    );
    assert_eq!(
        library.resolve("Albums/Empty").await,
        Err(VirtualLibraryError::NotAFile)
    );
    assert_eq!(
        library.list("Albums/Empty/a.jpg/deeper").await,
        Err(VirtualLibraryError::NotFound)
    );
}

#[tokio::test]
async fn albums_report_an_unreachable_server_without_credentials() {
    let server = serve_routes(|_: &RecordedRequest| ("401 Unauthorized".into(), "{}".into())).await;
    let client = test_client(server.url.clone(), "secret-api-key");
    let library = VirtualLibrary::new(&client);

    for path in ["Albums", "Albums/Holiday"] {
        let Err(VirtualLibraryError::Remote(failure)) = library.list(path).await else {
            panic!("expected a remote error for {path}");
        };
        assert_eq!(failure.kind, RemoteFailureKind::Unauthorized, "{path}");
        assert!(!failure.message.contains("secret-api-key"));
    }
}

#[tokio::test]
async fn a_live_photo_in_an_album_is_a_still_and_a_video_sharing_one_base_name() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let assets = vec![
        FakeAsset {
            live_video: Some("v-live"),
            size: 3_000,
            ..in_march("s-live", "IMG_0001.HEIC")
        },
        live_video("v-live", "IMG_0001.MOV", 9_000),
        // An Archived Live Photo stays in the Album, video included.
        FakeAsset {
            live_video: Some("v-old"),
            visibility: "archive",
            ..in_march("s-old", "IMG_0002.HEIC")
        },
        live_video("v-old", "IMG_0002.MOV", 8_000),
        in_march("p1", "IMG_0003.jpg"),
        in_march("elsewhere", "IMG_0004.jpg"),
    ];
    let albums = vec![album(
        "al1",
        "Live",
        &["s-live", "v-live", "s-old", "v-old", "p1"],
    )];
    let server = fake_immich(assets, albums).await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let live = library.list("Albums/Live").await.unwrap();

    // Each video shows once, beside its still, and never as a file of its own.
    assert_eq!(
        names(&live),
        [
            "IMG_0001.HEIC",
            "IMG_0001.MOV",
            "IMG_0002.HEIC",
            "IMG_0002.MOV",
            "IMG_0003.jpg"
        ]
    );
    assert_eq!(live[0].size, Some(3_000));
    assert_eq!(live[1].size, Some(9_000));
    for (path, id) in [
        ("Albums/Live/IMG_0001.HEIC", "s-live"),
        ("Albums/Live/IMG_0001.MOV", "v-live"),
        ("Albums/Live/IMG_0002.HEIC", "s-old"),
        ("Albums/Live/IMG_0002.MOV", "v-old"),
    ] {
        assert_eq!(
            library.resolve(path).await,
            Ok(RemoteAssetId::new(id)),
            "{path}"
        );
    }
}

#[tokio::test]
async fn names_illegal_on_ntfs_are_listed_as_safe_readable_names_that_resolve() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        in_march("i1", "a<b>:c\"d|e?f*.jpg"),
        in_march("i2", "tab\there.jpg"),
        in_march("r1", "CON.txt"),
        in_march("r2", "nul"),
        in_march("r3", "Com1.jpg"),
        in_march("r4", "LPT9.tar.gz"),
        in_march("t1", "holiday.jpg. "),
        in_march("t2", "notes "),
        in_march("e1", ".."),
        // Names that merely contain a reserved word stay as they are.
        in_march("k1", "console.jpg"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        [
            "_",
            "_Com1.jpg",
            "_CON.txt",
            "_LPT9.tar.gz",
            "_nul",
            "a_b__c_d_e_f_.jpg",
            "console.jpg",
            "holiday.jpg",
            "notes",
            "tab_here.jpg",
        ]
    );
    for (path, id) in [
        ("All Photos/2025/03 March/_CON.txt", "r1"),
        ("All Photos/2025/03 March/a_b__c_d_e_f_.jpg", "i1"),
        ("All Photos/2025/03 March/holiday.jpg", "t1"),
        ("All Photos/2025/03 March/_", "e1"),
    ] {
        assert_eq!(
            library.resolve(path).await,
            Ok(RemoteAssetId::new(id)),
            "{path}"
        );
    }
}

/// Immich ids are UUIDs; the name suffix is the last six characters of one.
const ID_A1B2C3: &str = "01930000-0000-7000-8000-0000000a1b2c3";
const ID_D4E5F6: &str = "01930000-0000-7000-8000-0000000d4e5f6";

#[tokio::test]
async fn assets_with_one_name_on_one_day_are_listed_with_a_suffix_from_their_id() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        in_march(ID_A1B2C3, "IMG_0001.jpg"),
        in_march(ID_D4E5F6, "IMG_0001.jpg"),
        // Windows names compare without regard to case.
        in_march("c1", "img_0001.JPG"),
        in_march("p1", "IMG_0002.jpg"),
        // The same name on another day is no collision.
        asset("o1", "IMG_0002.jpg", "2025-04-10T10:00:00.000Z"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        [
            "IMG_0001 (a1b2c3).jpg",
            "img_0001 (c1).JPG",
            "IMG_0001 (d4e5f6).jpg",
            "IMG_0002.jpg"
        ]
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0001 (d4e5f6).jpg")
            .await,
        Ok(RemoteAssetId::new(ID_D4E5F6))
    );
    assert_eq!(
        names(&library.list("All Photos/2025/04 April").await.unwrap()),
        ["IMG_0002.jpg"]
    );
}

#[tokio::test]
async fn albums_with_one_name_are_listed_with_a_suffix_and_open_to_their_own_assets() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_immich(
        vec![in_march("p1", "mine.jpg"), in_march("p2", "yours.jpg")],
        vec![
            album(ID_A1B2C3, "Trip 2.0", &["p1"]),
            // Owned by someone else and shared with the user.
            FakeAlbum {
                owned: false,
                shared: true,
                ..album(ID_D4E5F6, "trip 2.0", &["p2"])
            },
            album("al3", "Holiday", &[]),
        ],
    )
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let listed = library.list("Albums").await.unwrap();

    // A name held once stays plain; the suffix goes at the end, a folder has no extension.
    assert_eq!(
        names(&listed),
        ["Holiday", "Trip 2.0 (a1b2c3)", "trip 2.0 (d4e5f6)"]
    );
    assert_eq!(
        names(&library.list("Albums/Trip 2.0 (a1b2c3)").await.unwrap()),
        ["mine.jpg"]
    );
    assert_eq!(
        names(&library.list(r"Albums\trip 2.0 (d4e5f6)").await.unwrap()),
        ["yours.jpg"]
    );
    assert_eq!(
        library.list("Albums/Trip 2.0").await,
        Err(VirtualLibraryError::NotFound)
    );
}

#[tokio::test]
async fn album_names_illegal_on_ntfs_are_listed_and_opened_by_their_safe_name() {
    let server = fake_immich(
        vec![asset("p1", "a.jpg", "2025-03-10T10:00:00.000Z")],
        vec![
            album("al1", "Trip: Italy/2025?", &["p1"]),
            album("al2", "Summer.", &[]),
            album("al3", "CON", &[]),
            album("al4", "  ", &[]),
        ],
    )
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let listed = library.list("Albums").await.unwrap();

    assert_eq!(names(&listed), ["_", "_CON", "Summer", "Trip_ Italy_2025_"]);
    assert_eq!(
        names(&library.list("Albums/Trip_ Italy_2025_").await.unwrap()),
        ["a.jpg"]
    );
    assert_eq!(
        library.resolve("Albums/Trip_ Italy_2025_/a.jpg").await,
        Ok(RemoteAssetId::new("p1"))
    );
    assert_eq!(
        library.list("Albums/_CON").await.unwrap(),
        Vec::<Entry>::new()
    );
}

#[tokio::test]
async fn names_stay_the_same_across_listings_and_when_unrelated_assets_are_added() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let before = vec![
        in_march(ID_A1B2C3, "IMG_0001.jpg"),
        in_march(ID_D4E5F6, "IMG_0001.jpg"),
        in_march("p1", "IMG_0002.jpg"),
    ];
    // Unrelated arrivals, some sorting before the existing ones by id, name and date.
    let mut after = before.clone();
    after.extend([
        in_march("00000000-0000-7000-8000-000000000001", "IMG_0000.jpg"),
        in_march("ffffffff-0000-7000-8000-ffffffffffff", "IMG_9999.jpg"),
        asset("n1", "IMG_0001.jpg", "2025-02-01T10:00:00.000Z"),
    ]);
    let listed = |assets: Vec<FakeAsset>| async move {
        let server = fake_library(assets).await;
        let client = test_client(server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);
        let first = library.list("All Photos/2025/03 March").await.unwrap();
        let second = library.list("All Photos/2025/03 March").await.unwrap();
        assert_eq!(first, second);
        first
    };

    let before = listed(before).await;
    let after = listed(after).await;

    for entry in &before {
        assert!(after.contains(entry), "{} changed", entry.name);
    }
    assert_eq!(
        names(&before),
        [
            "IMG_0001 (a1b2c3).jpg",
            "IMG_0001 (d4e5f6).jpg",
            "IMG_0002.jpg"
        ]
    );
}

#[tokio::test]
async fn the_order_immich_returns_assets_in_does_not_change_the_names() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let one = vec![
        in_march(ID_A1B2C3, "IMG_0001.jpg"),
        in_march(ID_D4E5F6, "img_0001.jpg"),
    ];
    let mut other = one.clone();
    other.reverse();
    let listed = |assets: Vec<FakeAsset>| async move {
        let server = fake_library(assets).await;
        let client = test_client(server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);
        library.list("All Photos/2025/03 March").await.unwrap()
    };

    assert_eq!(listed(one).await, listed(other).await);
}

#[tokio::test]
async fn a_suffixed_name_that_clashes_with_another_asset_takes_a_longer_suffix() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        in_march(ID_A1B2C3, "IMG_0001.jpg"),
        in_march(ID_D4E5F6, "IMG_0001.jpg"),
        in_march("lit", "IMG_0001 (a1b2c3).jpg"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        [
            "IMG_0001 (00a1b2c3).jpg",
            // The asset that is literally called this keeps it.
            "IMG_0001 (a1b2c3).jpg",
            "IMG_0001 (d4e5f6).jpg",
        ]
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0001 (a1b2c3).jpg")
            .await,
        Ok(RemoteAssetId::new("lit"))
    );
}

#[tokio::test]
async fn a_live_photo_pair_keeps_one_base_name_when_its_name_collides() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let live = |id, video, name| FakeAsset {
        live_video: Some(video),
        ..in_march(id, name)
    };
    let server = fake_library(vec![
        live(ID_A1B2C3, "va", "IMG_0001.HEIC"),
        live(ID_D4E5F6, "vd", "IMG_0001.HEIC"),
        live_video("va", "x.MOV", 1),
        live_video("vd", "y.MOV", 2),
        // A paired video's name also collides with a standalone video of that name.
        live("s3", "v3", "IMG_0002.HEIC"),
        live_video("v3", "z.MOV", 3),
        FakeAsset {
            file_created_at: "2025-03-10T10:00:00.000Z",
            ..in_march("m1", "IMG_0002.MOV")
        },
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        [
            "IMG_0001 (a1b2c3).HEIC",
            "IMG_0001 (a1b2c3).MOV",
            "IMG_0001 (d4e5f6).HEIC",
            "IMG_0001 (d4e5f6).MOV",
            "IMG_0002 (m1).MOV",
            "IMG_0002 (s3).HEIC",
            "IMG_0002 (s3).MOV",
        ]
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0001 (d4e5f6).MOV")
            .await,
        Ok(RemoteAssetId::new("vd"))
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0002 (m1).MOV")
            .await,
        Ok(RemoteAssetId::new("m1"))
    );
}

#[tokio::test]
async fn a_live_photo_pair_shares_its_suffix_when_only_its_still_collides() {
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        // The still shares its name with a plain photo; the video's own name is free.
        FakeAsset {
            live_video: Some("v1"),
            ..in_march(ID_A1B2C3, "IMG_0003.HEIC")
        },
        live_video("v1", "clip.MOV", 1),
        in_march(ID_D4E5F6, "IMG_0003.HEIC"),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    assert_eq!(
        names(&march),
        [
            "IMG_0003 (a1b2c3).HEIC",
            "IMG_0003 (a1b2c3).MOV",
            "IMG_0003 (d4e5f6).HEIC",
        ]
    );
    assert_eq!(
        library
            .resolve("All Photos/2025/03 March/IMG_0003 (a1b2c3).MOV")
            .await,
        Ok(RemoteAssetId::new("v1"))
    );
}

#[tokio::test]
async fn very_long_names_are_shortened_to_fit_with_their_extension_and_suffix() {
    let long = format!("{}.jpeg", "x".repeat(300));
    let emoji = format!("{}.png", "\u{1F600}".repeat(200));
    // The fake Library takes `'static` names; leaking two test strings is harmless.
    let long: &'static str = Box::leak(long.into_boxed_str());
    let emoji: &'static str = Box::leak(emoji.into_boxed_str());
    let in_march = |id, name| asset(id, name, "2025-03-10T10:00:00.000Z");
    let server = fake_library(vec![
        in_march(ID_A1B2C3, long),
        in_march(ID_D4E5F6, long),
        in_march("e1", emoji),
    ])
    .await;
    let client = test_client(server.url.clone(), "key");
    let library = VirtualLibrary::new(&client);

    let march = library.list("All Photos/2025/03 March").await.unwrap();

    let units = |name: &str| name.encode_utf16().count();
    assert_eq!(march.len(), 3);
    for entry in &march {
        assert!(units(&entry.name) <= 255, "{}", entry.name);
    }
    let suffixed: Vec<&str> = names(&march)
        .into_iter()
        .filter(|name| name.starts_with('x'))
        .collect();
    assert_eq!(suffixed.len(), 2);
    assert!(suffixed[0].ends_with(" (a1b2c3).jpeg"), "{}", suffixed[0]);
    assert!(suffixed[1].ends_with(" (d4e5f6).jpeg"), "{}", suffixed[1]);
    assert!(march.iter().any(|entry| entry.name.ends_with(".png")));
    assert_eq!(
        library
            .resolve(&format!("All Photos/2025/03 March/{}", suffixed[1]))
            .await,
        Ok(RemoteAssetId::new(ID_D4E5F6))
    );
}

/// Lazy, bounded listing (#271): a directory is produced from the requests its own contents
/// need, never from a projection of the whole Library.
mod lazy_listing {
    use super::*;

    fn leak(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    /// `count` assets captured around noon on days 2 to 28 of `year`-`month`.
    fn month_of(year: i32, month: u32, count: usize) -> Vec<FakeAsset> {
        (0..count)
            .map(|index| {
                let day = 2 + index % 27;
                asset(
                    leak(format!("a-{year}-{month:02}-{index:05}")),
                    leak(format!("IMG_{year}{month:02}_{index:05}.jpg")),
                    leak(format!("{year}-{month:02}-{day:02}T12:00:00.000Z")),
                )
            })
            .collect()
    }

    /// 24 months of 500 assets each: 12,000 Remote Assets.
    fn large_library() -> Vec<FakeAsset> {
        (2023..=2024)
            .flat_map(|year| (1..=12).flat_map(move |month| month_of(year, month, 500)))
            .collect()
    }

    struct Fixture {
        server: RoutedServer,
        library: Arc<Mutex<Vec<FakeAsset>>>,
        served: Arc<AtomicUsize>,
    }

    async fn fixture(assets: Vec<FakeAsset>, albums: Vec<FakeAlbum>) -> Fixture {
        let library = Arc::new(Mutex::new(assets));
        let served = Arc::new(AtomicUsize::new(0));
        let server = fake_immich_shared(library.clone(), albums, served.clone()).await;
        Fixture {
            server,
            library,
            served,
        }
    }

    impl Fixture {
        /// How many assets the server has sent in search pages so far.
        fn served(&self) -> usize {
            self.served.load(Ordering::SeqCst)
        }

        /// The bodies of the metadata searches received so far.
        fn searches(&self) -> Vec<Value> {
            self.server
                .requests()
                .iter()
                .filter(|request| request.target == "/api/search/metadata")
                .map(|request| serde_json::from_str(&request.body).unwrap())
                .collect()
        }

        /// The method and target of every request that is not a metadata search.
        fn other_requests(&self) -> Vec<String> {
            self.server
                .requests()
                .iter()
                .filter(|request| request.target != "/api/search/metadata")
                .map(|request| format!("{} {}", request.method, request.target))
                .collect()
        }
    }

    fn window_of(search: &Value) -> chrono::Duration {
        let bound = |key: &str| {
            DateTime::parse_from_rfc3339(search[key].as_str().expect(key)).expect("an instant")
        };
        bound("takenBefore") - bound("takenAfter")
    }

    #[tokio::test]
    async fn listing_one_month_of_a_large_library_fetches_only_that_month() {
        let fixture = fixture(large_library(), vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let june = library.list("All Photos/2024/06 June").await.unwrap();

        assert_eq!(june.len(), 500);
        assert!(june
            .iter()
            .all(|entry| entry.name.starts_with("IMG_202406_")));
        // The server sent that month and nothing else of the 12,000 assets.
        assert_eq!(fixture.served(), 500);
        // One search page, bounded to the month (with room for the local-time offset), and no
        // other request: no other month, no Album, no timeline buckets.
        let searches = fixture.searches();
        assert_eq!(searches.len(), 1);
        assert!(window_of(&searches[0]) <= chrono::Duration::days(33));
        assert!(searches[0].get("albumIds").is_none());
        assert!(fixture.other_requests().is_empty());
    }

    #[tokio::test]
    async fn resolving_a_file_reads_only_the_directory_that_holds_it() {
        let fixture = fixture(large_library(), vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let resolved = library
            .resolve("All Photos/2023/02 February/IMG_202302_00007.jpg")
            .await;

        assert_eq!(resolved, Ok(RemoteAssetId::new("a-2023-02-00007")));
        assert_eq!(fixture.served(), 500);
        assert_eq!(fixture.searches().len(), 1);
    }

    #[tokio::test]
    async fn a_large_month_is_paginated_and_other_months_stay_unfetched() {
        let mut assets = month_of(2024, 6, 2_300);
        assets.extend(month_of(2024, 7, 400));
        assets.extend(month_of(2023, 1, 400));
        let fixture = fixture(assets, vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let june = library.list("All Photos/2024/06 June").await.unwrap();

        assert_eq!(june.len(), 2_300);
        assert_eq!(fixture.served(), 2_300);
        let searches = fixture.searches();
        let pages: Vec<u64> = searches
            .iter()
            .map(|search| search["page"].as_u64().unwrap())
            .collect();
        assert_eq!(pages, [1, 2, 3]);
        assert!(searches
            .iter()
            .all(|search| search["size"].as_u64().unwrap() <= 1_000));
    }

    #[tokio::test]
    async fn years_and_months_come_from_the_timeline_buckets_without_fetching_any_asset() {
        let fixture = fixture(large_library(), vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let years = library.list("All Photos").await.unwrap();
        let months = library.list("All Photos/2024").await.unwrap();

        assert_eq!(names(&years), ["2023", "2024"]);
        assert_eq!(months.len(), 12);
        assert_eq!(months[11].name, "12 December");
        assert_eq!(fixture.served(), 0);
        assert!(fixture.searches().is_empty());
        assert!(fixture
            .other_requests()
            .iter()
            .all(|request| request.starts_with("GET /api/timeline/buckets")));
    }

    #[tokio::test]
    async fn albums_lists_the_albums_without_fetching_any_asset() {
        let albums = vec![
            album("al1", "Holiday", &["a-2024-06-00001"]),
            album("al2", "Family", &["a-2023-01-00002"]),
        ];
        let fixture = fixture(large_library(), albums).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let listed = library.list("Albums").await.unwrap();

        assert_eq!(names(&listed), ["Family", "Holiday"]);
        assert_eq!(fixture.served(), 0);
        assert!(fixture.searches().is_empty());
        assert!(fixture
            .other_requests()
            .iter()
            .all(|request| request.starts_with("GET /api/albums")));
    }

    #[tokio::test]
    async fn one_album_folder_fetches_just_the_assets_of_that_album() {
        let albums = vec![
            album("al1", "Holiday", &["a-2024-06-00001", "a-2023-01-00002"]),
            album("al2", "Family", &["a-2024-06-00003"]),
        ];
        let fixture = fixture(large_library(), albums).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let holiday = library.list("Albums/Holiday").await.unwrap();

        assert_eq!(
            names(&holiday),
            ["IMG_202301_00002.jpg", "IMG_202406_00001.jpg"]
        );
        assert_eq!(fixture.served(), 2);
        let searches = fixture.searches();
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0]["albumIds"], json!(["al1"]));
        // No timeline bucket, and no look-up of another Album.
        assert!(fixture
            .other_requests()
            .iter()
            .all(|request| request.starts_with("GET /api/albums")));
    }

    #[tokio::test]
    async fn an_album_is_paginated_instead_of_fetched_in_one_piece() {
        let assets = month_of(2024, 6, 2_100);
        let ids: Vec<&'static str> = assets.iter().map(|asset| asset.id).collect();
        let fixture = fixture(assets, vec![album("al1", "Big", &ids)]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        let big = library.list("Albums/Big").await.unwrap();

        assert_eq!(big.len(), 2_100);
        let pages: Vec<u64> = fixture
            .searches()
            .iter()
            .map(|search| search["page"].as_u64().unwrap())
            .collect();
        assert_eq!(pages, [1, 2, 3]);
    }

    #[tokio::test]
    async fn a_directory_can_be_listed_again_and_the_two_listings_diffed_on_their_own() {
        let fixture = fixture(large_library(), vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);
        let june = "All Photos/2024/06 June";

        let before = library.list(june).await.unwrap();
        assert_eq!(library.list(june).await.unwrap(), before);

        {
            let mut assets = fixture.library.lock().unwrap();
            assets.retain(|asset| asset.id != "a-2024-06-00010");
            assets.extend(month_of(2024, 6, 1).into_iter().map(|asset| FakeAsset {
                id: "a-new",
                name: "NEW.jpg",
                ..asset
            }));
        }
        let after = library.list(june).await.unwrap();

        let gone: Vec<&str> = before
            .iter()
            .filter(|entry| !after.contains(entry))
            .map(|entry| entry.name.as_str())
            .collect();
        let added: Vec<&str> = after
            .iter()
            .filter(|entry| !before.contains(entry))
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(gone, ["IMG_202406_00010.jpg"]);
        assert_eq!(added, ["NEW.jpg"]);
        // Every one of the three listings asked for June alone.
        assert_eq!(fixture.searches().len(), 3);
        assert!(fixture.other_requests().is_empty());
    }

    #[tokio::test]
    async fn assets_at_the_extreme_utc_offsets_are_listed_in_their_local_month() {
        let server = fake_library(vec![
            // 00:30 on 1 April at UTC+14: 10:30 on 31 March in UTC.
            FakeAsset {
                file_created_at: "2025-03-31T10:30:00.000Z",
                ..asset("east", "EAST.jpg", "2025-04-01T00:30:00.000Z")
            },
            // 23:30 on 31 March at UTC-12: 11:30 on 1 April in UTC.
            FakeAsset {
                file_created_at: "2025-04-01T11:30:00.000Z",
                ..asset("west", "WEST.jpg", "2025-03-31T23:30:00.000Z")
            },
        ])
        .await;
        let client = test_client(server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        assert_eq!(
            names(&library.list("All Photos/2025/03 March").await.unwrap()),
            ["WEST.jpg"]
        );
        assert_eq!(
            names(&library.list("All Photos/2025/04 April").await.unwrap()),
            ["EAST.jpg"]
        );
    }

    #[tokio::test]
    async fn undated_assets_are_found_without_projecting_the_whole_library_at_once() {
        let mut assets = month_of(2024, 6, 2_100);
        assets.push(FakeAsset {
            local_date_time: None,
            ..asset("u1", "scan.png", "")
        });
        let fixture = fixture(assets, vec![]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        assert_eq!(
            names(&library.list("All Photos").await.unwrap()),
            ["2024", "Undated"]
        );
        let undated = library.list("All Photos/Undated").await.unwrap();

        assert_eq!(names(&undated), ["scan.png"]);
        // Paged through in pieces of at most 1,000 assets.
        assert!(fixture
            .searches()
            .iter()
            .all(|search| search["size"].as_u64().unwrap() <= 1_000));
    }

    /// ADR-0003: the Virtual Library never causes an Immich write. Walking every kind of directory
    /// and resolving a file sends only reads: `GET`s, and the metadata search, which Immich models as
    /// a `POST` although it changes nothing.
    #[tokio::test]
    async fn walking_the_whole_projection_sends_only_reads_to_immich() {
        let mut assets = month_of(2024, 6, 3);
        assets.push(FakeAsset {
            local_date_time: None,
            ..asset("u1", "scan.png", "")
        });
        let ids: Vec<&'static str> = assets.iter().map(|asset| asset.id).collect();
        let fixture = fixture(assets, vec![album("al1", "Holiday", &ids)]).await;
        let client = test_client(fixture.server.url.clone(), "key");
        let library = VirtualLibrary::new(&client);

        for directory in [
            "",
            "All Photos",
            "All Photos/2024",
            "All Photos/2024/06 June",
            "All Photos/Undated",
            "Albums",
            "Albums/Holiday",
        ] {
            library.list(directory).await.unwrap();
        }
        library
            .resolve("All Photos/Undated/scan.png")
            .await
            .unwrap();

        let requests = fixture.server.requests();
        assert!(!requests.is_empty());
        for request in requests {
            let is_read = request.method == "GET"
                || (request.method == "POST" && request.target == "/api/search/metadata");
            assert!(
                is_read,
                "the Virtual Library sent {} {}",
                request.method, request.target
            );
        }
    }
}
