use crate::immich::{ImmichSession, SearchRequest, SearchResults, SearchSupport};

/// Whether the connected Immich Server is new enough (3.2) to answer a Search.
#[tauri::command]
pub(crate) async fn get_search_support() -> Result<SearchSupport, String> {
    ImmichSession::with_stored_credentials(
        |client| async move { client.get_search_support().await },
    )
    .await
}

/// A chronological Search over the whole Immich Library. Read-only.
#[tauri::command]
pub(crate) async fn search_assets(request: SearchRequest) -> Result<SearchResults, String> {
    ImmichSession::with_stored_credentials(
        |client| async move { client.search_assets(&request).await },
    )
    .await
}
