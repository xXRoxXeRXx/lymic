use crate::immich::{ImmichSession, PeoplePage};

#[tauri::command]
pub(crate) async fn get_people(page: u32, size: u32) -> Result<PeoplePage, String> {
    let session = ImmichSession::from_stored_credentials()?;
    session
        .run(|client| async move { client.get_people(page.max(1), size.clamp(1, 100)).await })
        .await
}
