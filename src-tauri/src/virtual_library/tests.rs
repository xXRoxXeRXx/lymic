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
    serve_routes(move |request: &RecordedRequest| {
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
        let items: Vec<Value> = assets
            .iter()
            .filter(|asset| {
                visibility.map_or(asset.visibility != "locked", |v| asset.visibility == v)
            })
            .filter(|asset| with_deleted || !asset.trashed)
            .filter(|asset| in_albums(asset))
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
        let Err(VirtualLibraryError::Remote(message)) = library.list(path).await else {
            panic!("expected a remote error for {path}");
        };
        assert!(!message.contains("secret-api-key"));
    }
}
