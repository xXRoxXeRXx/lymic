use crate::immich::{ImmichSession, Memory};

#[tauri::command]
pub(crate) async fn get_memories(size: u32) -> Result<Vec<Memory>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_memories(size.clamp(1, 100)).await
    })
    .await
}
