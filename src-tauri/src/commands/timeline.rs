use crate::immich::{ImmichClient, PersonTimelinePage, TimelineAsset, TimelineBucket};

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
pub(crate) async fn get_person_timeline_assets(
    person_id: String,
    page: u32,
    size: u32,
) -> Result<PersonTimelinePage, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_person_timeline_assets(&person_id, page.max(1), size.clamp(1, 250))
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}
