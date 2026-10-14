use crate::auth;
use crate::immich::{Album, ImmichClient};

#[tauri::command]
pub(crate) async fn get_albums() -> Result<Vec<Album>, String> {
    let credentials = auth::get_credentials()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not authenticated".to_string())?;

    let client = ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?;

    client.get_albums().await
}

#[tauri::command]
pub(crate) async fn create_album(name: String) -> Result<Album, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Album name cannot be empty".to_string());
    }

    let credentials = auth::get_credentials()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not authenticated".to_string())?;

    let client = ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?;

    client.create_album(trimmed).await
}
