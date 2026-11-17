use crate::immich::{ImmichSession, SuggestionRequest};

/// Values Immich suggests for a Search Filter (camera make, model, lens model; more types
/// follow with their filters), optionally narrowed. Read-only.
#[tauri::command]
pub(crate) async fn get_search_suggestions(
    request: SuggestionRequest,
) -> Result<Vec<String>, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.get_search_suggestions(&request).await
    })
    .await
}
