use std::path::Path;

use tauri_plugin_dialog::DialogExt;

use crate::immich::{AlbumTimeline, AssetDetails, ImmichClient, TimelineAsset, TimelineBucket};

#[tauri::command]
pub(crate) async fn get_asset_details(asset_id: String) -> Result<AssetDetails, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_asset_details(&asset_id)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn set_asset_favorite(asset_id: String, is_favorite: bool) -> Result<(), String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .set_asset_favorite(&asset_id, is_favorite)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn download_asset(
    asset_id: String,
    destination_path: String,
) -> Result<(), String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .download_asset(&asset_id, Path::new(&destination_path))
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn download_archive(
    app: tauri::AppHandle,
    asset_ids: Vec<String>,
) -> Result<(), String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    let archive = client
        .prepare_archive_download(&asset_ids)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))?;
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
    archive
        .save_to(destination_path)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_album_timeline(album_id: String) -> Result<AlbumTimeline, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_album_timeline(&album_id)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_timeline_buckets()
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_timeline_bucket_assets(
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_timeline_bucket_assets(&time_bucket)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_favorites_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_favorites_timeline_buckets()
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_favorites_timeline_bucket_assets(
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_favorites_timeline_bucket_assets(&time_bucket)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_person_timeline_buckets(
    person_id: String,
) -> Result<Vec<TimelineBucket>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_person_timeline_buckets(&person_id)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_person_timeline_bucket_assets(
    person_id: String,
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_person_timeline_bucket_assets(&person_id, &time_bucket)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}
