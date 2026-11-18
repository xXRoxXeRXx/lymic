use std::path::{Path, PathBuf};

use tauri_plugin_dialog::DialogExt;

use crate::immich::{
    AlbumTimeline, AssetDetails, AssetVisibility, AssetVisibilityResult, FilteredTimeline,
    ImmichSession, PlaceTimeline, RemoteAssetId, TimelineAsset, TimelineBucket, TrashRestoreResult,
};

#[tauri::command]
pub(crate) async fn get_asset_details(asset_id: String) -> Result<AssetDetails, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_asset_details(&asset_id).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_asset_favorite(asset_id: String, is_favorite: bool) -> Result<(), String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.set_asset_favorite(&asset_id, is_favorite).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn download_asset(
    asset_id: String,
    destination_path: String,
) -> Result<(), String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .download_asset(&asset_id, Path::new(&destination_path))
            .await
    })
    .await
}

#[tauri::command]
pub(crate) async fn download_archive(
    app: tauri::AppHandle,
    asset_ids: Vec<String>,
) -> Result<(), String> {
    let session = ImmichSession::from_stored_credentials()?;
    download_archive_with(&session, asset_ids, |suggested_file_name| {
        let Some(destination) = app
            .dialog()
            .file()
            .set_file_name(suggested_file_name)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        destination
            .as_path()
            .map(|path| Some(path.to_path_buf()))
            .ok_or_else(|| "Selected download destination is not a local path".to_string())
    })
    .await
}

/// Prepares Immich's archive, offers its suggested filename to `choose_destination`
/// (which returns `None` when the user cancels), and saves it there.
async fn download_archive_with(
    session: &ImmichSession,
    asset_ids: Vec<String>,
    choose_destination: impl FnOnce(&str) -> Result<Option<PathBuf>, String>,
) -> Result<(), String> {
    let archive = session
        .run(|client| async move { client.prepare_archive_download(&asset_ids).await })
        .await?;
    let Some(destination_path) = choose_destination(archive.suggested_file_name())? else {
        return Ok(());
    };
    session.save_archive(archive, &destination_path).await
}

/// Turns the IDs a command received into Remote Asset IDs: at least one, each trimmed and non-empty.
fn remote_asset_ids(asset_ids: &[String]) -> Result<Vec<RemoteAssetId>, String> {
    if asset_ids.is_empty() {
        return Err("At least one asset ID is required".to_string());
    }
    asset_ids
        .iter()
        .map(|asset_id| {
            let asset_id = asset_id.trim();
            if asset_id.is_empty() {
                Err("Asset ID cannot be empty".to_string())
            } else {
                Ok(RemoteAssetId::new(asset_id))
            }
        })
        .collect()
}

/// Restores Trashed Assets to the Immich Library. Lymic offers no other write to the Immich trash.
#[tauri::command]
pub(crate) async fn restore_trashed_assets(
    asset_ids: Vec<String>,
) -> Result<TrashRestoreResult, String> {
    let asset_ids = remote_asset_ids(&asset_ids)?;
    ImmichSession::with_stored_credentials(|client| async move {
        client.restore_trashed_assets(&asset_ids).await
    })
    .await
}

/// Archives or Unarchives Remote Assets. `visibility` accepts only `archive` and `timeline`, so
/// Lymic can never write `locked` or `hidden`.
#[tauri::command]
pub(crate) async fn set_assets_visibility(
    asset_ids: Vec<String>,
    visibility: AssetVisibility,
) -> Result<AssetVisibilityResult, String> {
    let asset_ids = remote_asset_ids(&asset_ids)?;
    ImmichSession::with_stored_credentials(|client| async move {
        client.set_assets_visibility(&asset_ids, visibility).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_album_timeline(album_id: String) -> Result<AlbumTimeline, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_album_timeline(&album_id).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_place_timeline(city: String) -> Result<PlaceTimeline, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_place_timeline(&city).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    ImmichSession::with_stored_credentials(
        |client| async move { client.get_timeline_buckets().await },
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_timeline_bucket_assets(
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_timeline_bucket_assets(&time_bucket).await
    })
    .await
}

/// Timeline buckets of a Filtered Timeline (`timeline`: `"favorites"`, `"archive"` or `"trash"`).
#[tauri::command]
pub(crate) async fn get_filtered_timeline_buckets(
    timeline: FilteredTimeline,
) -> Result<Vec<TimelineBucket>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_filtered_timeline_buckets(timeline).await
    })
    .await
}

/// The assets of one bucket of a Filtered Timeline.
#[tauri::command]
pub(crate) async fn get_filtered_timeline_bucket_assets(
    timeline: FilteredTimeline,
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .get_filtered_timeline_bucket_assets(timeline, &time_bucket)
            .await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_person_timeline_buckets(
    person_id: String,
) -> Result<Vec<TimelineBucket>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_person_timeline_buckets(&person_id).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_person_timeline_bucket_assets(
    person_id: String,
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .get_person_timeline_bucket_assets(&person_id, &time_bucket)
            .await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_tag_timeline_buckets(
    tag_id: String,
) -> Result<Vec<TimelineBucket>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_tag_timeline_buckets(&tag_id).await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_tag_timeline_bucket_assets(
    tag_id: String,
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .get_tag_timeline_bucket_assets(&tag_id, &time_bucket)
            .await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthConfig;
    use std::cell::RefCell;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn temporary_destination() -> PathBuf {
        std::env::temp_dir().join(format!("lymic-archive-{}.zip", uuid::Uuid::new_v4()))
    }

    /// Serves one archive response: `declared_length` bytes promised, `body` actually sent.
    async fn serve_archive(declared_length: usize, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let _ = stream.read(&mut buffer).await.unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Disposition: attachment; filename=\"photos.zip\"\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn offers_the_filename_immich_suggests_and_saves_the_archive_there() {
        let config = AuthConfig::test_fixture(&serve_archive(7, b"archive").await);
        let session = ImmichSession::from_credentials(config).unwrap();
        let destination = temporary_destination();
        let offered = RefCell::new(None);

        download_archive_with(&session, vec!["asset-1".to_string()], |name| {
            *offered.borrow_mut() = Some(name.to_string());
            Ok(Some(destination.clone()))
        })
        .await
        .unwrap();

        assert_eq!(offered.borrow().as_deref(), Some("photos.zip"));
        assert_eq!(std::fs::read(&destination).unwrap(), b"archive");
        std::fs::remove_file(&destination).unwrap();
    }

    #[tokio::test]
    async fn a_cancelled_destination_chooser_ends_download_without_saving() {
        let config = AuthConfig::test_fixture(&serve_archive(7, b"archive").await);
        let session = ImmichSession::from_credentials(config).unwrap();

        let result =
            download_archive_with(&session, vec!["asset-1".to_string()], |_| Ok(None)).await;

        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn a_destination_chooser_error_is_returned_unchanged() {
        let config = AuthConfig::test_fixture(&serve_archive(7, b"archive").await);
        let session = ImmichSession::from_credentials(config).unwrap();

        let error = download_archive_with(&session, vec!["asset-1".to_string()], |_| {
            Err("Selected download destination is not a local path".to_string())
        })
        .await
        .unwrap_err();

        assert_eq!(error, "Selected download destination is not a local path");
    }

    #[tokio::test]
    async fn a_prepare_failure_is_credential_free_and_never_asks_for_a_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let config = AuthConfig::test_fixture(&format!("http://{address}"));
        let session = ImmichSession::from_credentials(config.clone()).unwrap();
        let asked = RefCell::new(false);

        let error = download_archive_with(&session, vec!["asset-1".to_string()], |_| {
            *asked.borrow_mut() = true;
            Ok(None)
        })
        .await
        .unwrap_err();

        assert!(error.starts_with("Failed to download archive"), "{error}");
        config.assert_credential_free(&error);
        assert!(!*asked.borrow());
    }

    #[tokio::test]
    async fn a_save_failure_is_credential_free_and_leaves_no_file() {
        let config = AuthConfig::test_fixture(&serve_archive(100, b"short").await);
        let session = ImmichSession::from_credentials(config.clone()).unwrap();
        let destination = temporary_destination();

        let error = download_archive_with(&session, vec!["asset-1".to_string()], |_| {
            Ok(Some(destination.clone()))
        })
        .await
        .unwrap_err();

        assert!(error.starts_with("Failed to download archive"), "{error}");
        config.assert_credential_free(&error);
        assert!(!destination.exists());
    }
}
