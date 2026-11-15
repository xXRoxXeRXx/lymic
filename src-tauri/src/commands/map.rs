use crate::immich::{ImmichSession, MapMarker};

#[tauri::command]
pub(crate) async fn get_map_markers() -> Result<Vec<MapMarker>, String> {
    let session = ImmichSession::from_stored_credentials()?;
    session
        .run(|client| async move { client.get_map_markers().await })
        .await
}
