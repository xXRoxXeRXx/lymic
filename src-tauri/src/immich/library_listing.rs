//! Lists Remote Assets for the Virtual Library projection (ADR-0003).
//!
//! The projection needs fields the timeline views never read: the original file name, the
//! local capture time and the file size. They come from Immich's flat metadata search, the
//! same endpoint the Album Timeline uses (ADR-0006).

use super::client::ImmichClient;
use super::ids::RemoteAssetId;
use super::response::{non_empty, read_response_body_limited, send_checked};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

const PAGE_SIZE: u64 = 1_000;
/// 1000 items with EXIF exceed the default 1 MiB response cap.
const MAX_PAGE_BYTES: usize = 32 * 1024 * 1024;

/// A Remote Asset as the Virtual Library sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAsset {
    pub id: RemoteAssetId,
    pub original_file_name: Option<String>,
    /// Immich's `localDateTime`: the capture's wall-clock time with a nominal `Z`. Its components
    /// are the local time; the offset is not meaningful. `None` when Immich has no capture time.
    pub local_date_time: Option<String>,
    pub file_created_at: Option<String>,
    pub file_modified_at: Option<String>,
    pub file_size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchItem {
    id: String,
    original_file_name: Option<String>,
    local_date_time: Option<String>,
    file_created_at: Option<String>,
    file_modified_at: Option<String>,
    visibility: Option<String>,
    #[serde(default)]
    is_trashed: bool,
    exif_info: Option<ExifInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExifInfo {
    file_size_in_byte: Option<u64>,
}

#[derive(Deserialize)]
struct SearchPage {
    assets: SearchPageAssets,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPageAssets {
    items: Vec<SearchItem>,
    #[serde(default, deserialize_with = "super::timeline::deserialize_next_page")]
    next_page: Option<u64>,
}

impl ImmichClient {
    /// Every Remote Asset of the Photo Timeline: Immich's `visibility=timeline`, which leaves out
    /// Archived Assets, locked-folder assets and Live Photo videos (#242). Trashed Assets are
    /// excluded by the search default and dropped again here, so a server that leaks one cannot
    /// make it appear in the Virtual Library.
    pub(crate) async fn list_photo_timeline_assets(&self) -> Result<Vec<LibraryAsset>, String> {
        self.search_library_assets(json!({ "visibility": "timeline" }))
            .await
    }

    /// Pages through the flat metadata search with `filter` until the result is complete.
    async fn search_library_assets(
        &self,
        filter: serde_json::Value,
    ) -> Result<Vec<LibraryAsset>, String> {
        let mut page = 1_u64;
        let mut assets = Vec::new();
        loop {
            let mut body = filter.clone();
            body["order"] = "desc".into();
            body["withExif"] = true.into();
            body["page"] = page.into();
            body["size"] = PAGE_SIZE.into();
            let response = send_checked(
                self.request(Method::POST, "search/metadata").json(&body),
                "Failed to list library assets",
            )
            .await?;
            let body = read_response_body_limited(response, MAX_PAGE_BYTES).await?;
            let payload: SearchPage = serde_json::from_slice(&body)
                .map_err(|error| format!("Invalid library listing response: {error}"))?;
            assets.extend(payload.assets.items.into_iter().filter_map(library_asset));
            match payload.assets.next_page {
                Some(next_page) if next_page > page => page = next_page,
                Some(_) => return Err("Invalid library listing pagination response".to_string()),
                None => return Ok(assets),
            }
        }
    }
}

/// `None` for an item that must never be projected: Trashed, or not in the Photo Timeline.
fn library_asset(item: SearchItem) -> Option<LibraryAsset> {
    let in_timeline = item.visibility.as_deref().is_none_or(|v| v == "timeline");
    if item.is_trashed || !in_timeline {
        return None;
    }
    Some(LibraryAsset {
        id: RemoteAssetId::new(item.id),
        original_file_name: non_empty(item.original_file_name),
        local_date_time: non_empty(item.local_date_time),
        file_created_at: non_empty(item.file_created_at),
        file_modified_at: non_empty(item.file_modified_at),
        file_size: item.exif_info.and_then(|exif| exif.file_size_in_byte),
    })
}
