use crate::immich::{ImmichClient, MapMarker};

#[tauri::command]
pub(crate) async fn get_map_markers() -> Result<Vec<MapMarker>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_map_markers()
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}
