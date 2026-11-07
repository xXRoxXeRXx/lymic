use crate::immich::{ImmichClient, TimelineAsset, TimelineBucket};

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
