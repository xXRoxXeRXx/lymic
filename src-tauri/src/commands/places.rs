use crate::immich::{ImmichSession, Place};

#[tauri::command]
pub(crate) async fn get_places() -> Result<Vec<Place>, String> {
    ImmichSession::with_stored_credentials(|client| async move { client.get_places().await }).await
}
