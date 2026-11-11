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
