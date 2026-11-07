use crate::auth;
use crate::immich::{ImmichClient, TimelineBucket};

#[tauri::command]
pub(crate) async fn get_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    let credentials = auth::get_credentials()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Not authenticated".to_string())?;
    let client = ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?;
    client.get_timeline_buckets().await
}
