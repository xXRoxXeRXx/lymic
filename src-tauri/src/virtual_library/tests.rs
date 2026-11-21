//! Seam 1: the public interface of the projection, driven against a fake Immich server.

use super::*;
use crate::immich::test_support::{serve_routes, test_client, RecordedRequest, RoutedServer};
use serde_json::{json, Value};

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
}

fn asset(id: &'static str, name: &'static str, local_date_time: &'static str) -> FakeAsset {
    FakeAsset {
        id,
        name,
        local_date_time: Some(local_date_time),
        file_created_at: "2025-01-01T00:00:00.000Z",
        size: 1_000,
        visibility: "timeline",
        trashed: false,
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
        item
    }
}

/// An Immich server that holds `assets` and answers metadata searches the way Immich does:
/// without a `visibility` filter every asset outside the locked folder matches (Archived
/// ones too), Trashed Assets only match with `withDeleted`.
async fn fake_library(assets: Vec<FakeAsset>) -> RoutedServer {
    serve_routes(move |request: &RecordedRequest| {
        if request.method != "POST" || request.target != "/api/search/metadata" {
            return ("404 Not Found".into(), "{}".into());
        }
        let body: Value = serde_json::from_str(&request.body).unwrap();
        let visibility = body["visibility"].as_str();
        let with_deleted = body["withDeleted"] == json!(true);
        let items: Vec<Value> = assets
            .iter()
            .filter(|asset| {
                visibility.map_or(asset.visibility != "locked", |v| asset.visibility == v)
            })
            .filter(|asset| with_deleted || !asset.trashed)
            .map(FakeAsset::json)
            .collect();
        let page = json!({ "assets": { "items": items, "nextPage": null, "total": items.len() } });
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

    let Err(VirtualLibraryError::Remote(message)) = library.list("All Photos").await else {
        panic!("expected a remote error");
    };
    assert!(!message.contains("secret-api-key"));
    // The root and Albums structure need no server.
    assert!(library.list("").await.is_ok());
}
