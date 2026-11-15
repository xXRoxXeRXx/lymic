use crate::immich::{ImmichSession, MemoriesPage, Memory};

#[tauri::command]
pub(crate) async fn get_memories(size: u32) -> Result<Vec<Memory>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_memories(size.clamp(1, 100)).await
    })
    .await
}

/// One page of the All Memories view. `page` is 1-based.
#[tauri::command]
pub(crate) async fn get_memories_page(page: u32, size: u32) -> Result<MemoriesPage, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client
            .get_memories_page(page.max(1), size.clamp(1, 100))
            .await
    })
    .await
}
