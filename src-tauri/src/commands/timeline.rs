use crate::auth;
use crate::immich::{ImmichClient, TimelineAsset, TimelineBucket};

fn authenticated_client() -> Result<ImmichClient, String> {
    let credentials = auth::get_credentials()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Not authenticated".to_string())?;
    ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )
}

#[tauri::command]
pub(crate) async fn get_timeline_buckets() -> Result<Vec<TimelineBucket>, String> {
    authenticated_client()?.get_timeline_buckets().await
}

#[tauri::command]
pub(crate) async fn get_timeline_bucket_assets(
    time_bucket: String,
) -> Result<Vec<TimelineAsset>, String> {
    authenticated_client()?
        .get_timeline_bucket_assets(&time_bucket)
        .await
}
