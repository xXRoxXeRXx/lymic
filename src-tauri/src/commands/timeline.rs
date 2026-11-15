use std::path::Path;

use tauri_plugin_dialog::DialogExt;

use crate::immich::{AlbumTimeline, AssetDetails, ImmichSession, TimelineAsset, TimelineBucket};

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
    let archive = session
        .run(|client| async move { client.prepare_archive_download(&asset_ids).await })
        .await?;
    let Some(destination_path) = app
        .dialog()
        .file()
        .set_file_name(archive.suggested_file_name())
        .blocking_save_file()
    else {
        return Ok(());
    };
    let destination_path = destination_path
        .as_path()
        .ok_or_else(|| "Selected download destination is not a local path".to_string())?;
    session.save_archive(archive, destination_path).await
}

#[tauri::command]
pub(crate) async fn get_album_timeline(album_id: String) -> Result<AlbumTimeline, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_album_timeline(&album_id).await
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

#[tauri::command]
pub(crate) async fn get_favorites_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_favorites_timeline_buckets().await
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_favorites_timeline_bucket_assets(
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .get_favorites_timeline_bucket_assets(&time_bucket)
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
