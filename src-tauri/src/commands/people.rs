use crate::immich::{ImmichClient, PeoplePage};

#[tauri::command]
pub(crate) async fn get_people(page: u32, size: u32) -> Result<PeoplePage, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_people(page.max(1), size.clamp(1, 100))
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}
