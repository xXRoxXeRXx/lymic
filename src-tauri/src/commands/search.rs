use crate::immich::{
    ContextSearchPage, ImmichSession, SearchFeatures, SearchQuery, SearchSupport, SearchTimeline,
};

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
pub(crate) async fn search_assets(query: SearchQuery) -> Result<SearchTimeline, String> {
    ImmichSession::with_stored_credentials(
        |client| async move { client.search_assets(&query).await },
    )
    .await
}

/// Which optional Search Modes (Context, OCR) the connected Immich Server has switched on.
#[tauri::command]
pub(crate) async fn get_search_features() -> Result<SearchFeatures, String> {
    ImmichSession::with_stored_credentials(
        |client| async move { client.get_search_features().await },
    )
    .await
}

/// One page of a Context Search: the Search Results in order of relevance. Read-only.
#[tauri::command]
pub(crate) async fn search_context_page(
    query: SearchQuery,
    page: u64,
) -> Result<ContextSearchPage, String> {
    ImmichSession::with_stored_credentials(|client| async move {
        client.search_context_page(&query, page).await
    })
    .await
}
