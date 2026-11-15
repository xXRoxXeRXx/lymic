use crate::immich::{ImmichSession, MapMarker};

#[tauri::command]
pub(crate) async fn get_map_markers() -> Result<Vec<MapMarker>, String> {
    ImmichSession::with_stored_credentials(|client| async move { client.get_map_markers().await })
        .await
}
