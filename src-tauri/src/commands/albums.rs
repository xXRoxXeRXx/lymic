use crate::immich::{
    Album, AlbumAssignmentResult, AlbumCatalogueItem, AlbumId, ImmichClient, RemoteAssetId,
};

#[tauri::command]
pub(crate) async fn get_album_catalogue() -> Result<Vec<AlbumCatalogueItem>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_album_catalogue()
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn get_albums() -> Result<Vec<Album>, String> {
    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .get_albums()
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn create_album(name: String) -> Result<Album, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Album name cannot be empty".to_string());
    }

    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .create_album(trimmed)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}

#[tauri::command]
pub(crate) async fn add_assets_to_album(
    asset_ids: Vec<String>,
    album_id: String,
) -> Result<AlbumAssignmentResult, String> {
    if asset_ids.is_empty() {
        return Err("At least one asset ID is required".to_string());
    }
    let asset_ids = asset_ids
        .iter()
        .map(|asset_id| {
            let asset_id = asset_id.trim();
            if asset_id.is_empty() {
                Err("Asset ID cannot be empty".to_string())
            } else {
                Ok(RemoteAssetId::new(asset_id))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let (client, credentials, _) = ImmichClient::from_stored_credentials()?;
    client
        .add_assets_to_album_with_result(&AlbumId::new(album_id), &asset_ids)
        .await
        .map_err(|error| ImmichClient::redact_error(&error, &credentials))
}
