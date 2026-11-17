use crate::immich::{ImmichSession, Tag};

#[tauri::command]
pub(crate) async fn get_tags() -> Result<Vec<Tag>, String> {
    ImmichSession::with_stored_credentials(|client| async move { client.get_tags().await }).await
}
