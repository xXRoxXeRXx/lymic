//! Lists Remote Assets for the Virtual Library projection (ADR-0003).
//!
//! The projection needs fields the timeline views never read: the original file name, the
//! local capture time and the file size. They come from Immich's flat metadata search, the
//! same endpoint the Album Timeline uses (ADR-0006).

use super::client::ImmichClient;
use super::ids::{AlbumId, RemoteAssetId};
use super::response::{
    non_empty, read_response_body_limited, send_checked, MAX_SUCCESS_RESPONSE_BYTES,
};
use super::timeline::TimelineBucket;
use chrono::{DateTime, SecondsFormat, Utc};
use futures::{stream, StreamExt};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

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
    /// Immich's `livePhotoVideoId`: set on the still of a Live Photo, naming the separate video
    /// asset (hidden from the timeline) that completes the pair.
    pub live_photo_video_id: Option<RemoteAssetId>,
}

/// An Album as the Virtual Library sees it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryAlbum {
    pub id: AlbumId,
    pub album_name: String,
}

/// The video half of a Live Photo, as far as the projection needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePhotoVideo {
    pub original_file_name: Option<String>,
    pub file_size: Option<u64>,
}

/// How many Live Photo videos are looked up at once.
const VIDEO_LOOKUP_CONCURRENCY: usize = 8;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchItem {
    id: String,
    original_file_name: Option<String>,
    local_date_time: Option<String>,
    file_created_at: Option<String>,
    file_modified_at: Option<String>,
    visibility: Option<String>,
    live_photo_video_id: Option<String>,
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
    /// The months of the Photo Timeline with their asset counts: Immich's `timeline/buckets` with
    /// `visibility=timeline`, which leaves out Archived Assets, locked-folder assets and Live
    /// Photo videos (#242). One small response however many assets the Library holds, so the year
    /// and month directories of `All Photos` need no asset at all. Immich buckets by local
    /// capture time, like the projection.
    pub(crate) async fn list_photo_timeline_months(&self) -> Result<Vec<TimelineBucket>, String> {
        self.get_timeline_buckets().await
    }

    /// The Photo Timeline assets whose `fileCreatedAt` (UTC) lies in `[after, before]`, paginated.
    /// Immich's `takenAfter` and `takenBefore` bound that UTC instant, not the local capture time
    /// the projection files assets by, so the caller widens the window by the largest UTC offset
    /// and drops what falls outside its local month. Trashed Assets are excluded by the search
    /// default and dropped again here, so a server that leaks one cannot make it appear in the
    /// Virtual Library.
    pub(crate) async fn list_photo_timeline_assets_created_between(
        &self,
        after: DateTime<Utc>,
        before: DateTime<Utc>,
    ) -> Result<Vec<LibraryAsset>, String> {
        let filter = json!({
            "visibility": "timeline",
            "takenAfter": after.to_rfc3339_opts(SecondsFormat::Millis, true),
            "takenBefore": before.to_rfc3339_opts(SecondsFormat::Millis, true),
        });
        self.search_library_assets(filter, true, &|_| true).await
    }

    /// The Photo Timeline assets `keep` accepts, found by paging through the whole timeline and
    /// holding on to nothing else: memory stays proportional to the result and one page, but the
    /// work is a full pass. Only for what Immich cannot filter on, such as a missing capture time.
    pub(crate) async fn find_photo_timeline_assets(
        &self,
        keep: &(dyn Fn(&LibraryAsset) -> bool + Sync),
    ) -> Result<Vec<LibraryAsset>, String> {
        self.search_library_assets(json!({ "visibility": "timeline" }), true, keep)
            .await
    }

    /// Every Remote Asset Immich returns for the Album: Archived Assets included, Trashed ones
    /// never (the search default, enforced again here).
    pub(crate) async fn list_album_assets(
        &self,
        album_id: &AlbumId,
    ) -> Result<Vec<LibraryAsset>, String> {
        self.search_library_assets(json!({ "albumIds": [album_id] }), false, &|_| true)
            .await
    }

    /// The Albums the user owns plus the Albums shared with the user, each once, in Immich's
    /// order. `GET /albums` alone lists only the Albums the user owns; the Albums shared with the
    /// user come with `shared=true`, which Immich's own web client requests as well.
    pub(crate) async fn list_library_albums(&self) -> Result<Vec<LibraryAlbum>, String> {
        let mut albums = self.fetch_library_albums(false).await?;
        for album in self.fetch_library_albums(true).await? {
            if !albums.iter().any(|known| known.id == album.id) {
                albums.push(album);
            }
        }
        Ok(albums)
    }

    async fn fetch_library_albums(&self, shared: bool) -> Result<Vec<LibraryAlbum>, String> {
        let mut request = self.request(Method::GET, "albums");
        if shared {
            request = request.query(&[("shared", "true")]);
        }
        let response = send_checked(request, "Failed to list library albums").await?;
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid library albums response: {error}"))
    }

    /// The video halves of the Live Photos among `assets`, by video id. Immich lists the video as
    /// an asset of its own that no timeline search returns, so each one is looked up. A video
    /// that cannot be looked up (deleted, or not readable) is left out, which leaves its still
    /// to be projected alone rather than failing the whole listing.
    pub(crate) async fn list_live_photo_videos(
        &self,
        assets: &[LibraryAsset],
    ) -> HashMap<RemoteAssetId, LivePhotoVideo> {
        // Owned ids, so the lookups do not borrow from the listing: a borrowing closure makes
        // this future impossible to prove `Send`, which the Windows adapter needs.
        let ids: BTreeSet<RemoteAssetId> = assets
            .iter()
            .filter_map(|asset| asset.live_photo_video_id.clone())
            .collect();
        stream::iter(ids)
            .map(|id| async move {
                let details = self.get_asset_details(id.as_str()).await.ok()?;
                let video = LivePhotoVideo {
                    original_file_name: details.original_file_name,
                    file_size: details.file_size,
                };
                Some((id, video))
            })
            .buffer_unordered(VIDEO_LOOKUP_CONCURRENCY)
            .filter_map(|found| async move { found })
            .collect()
            .await
    }

    /// Pages through the flat metadata search with `filter` until the result is complete, one
    /// page of at most `PAGE_SIZE` assets at a time. `timeline_only` drops what is not in the
    /// Photo Timeline (Archived, hidden); Trashed Assets are always dropped; `keep` drops the
    /// rest of each page before the next one is requested.
    async fn search_library_assets(
        &self,
        filter: serde_json::Value,
        timeline_only: bool,
        keep: &(dyn Fn(&LibraryAsset) -> bool + Sync),
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
            assets.extend(
                payload
                    .assets
                    .items
                    .into_iter()
                    .filter_map(|item| library_asset(item, timeline_only))
                    .filter(|asset| keep(asset)),
            );
            match payload.assets.next_page {
                Some(next_page) if next_page > page => page = next_page,
                Some(_) => return Err("Invalid library listing pagination response".to_string()),
                None => return Ok(assets),
            }
        }
    }
}

/// `None` for an item that must never be projected: Trashed, or, with `timeline_only`, not in
/// the Photo Timeline.
fn library_asset(item: SearchItem, timeline_only: bool) -> Option<LibraryAsset> {
    let in_timeline = item.visibility.as_deref().is_none_or(|v| v == "timeline");
    if item.is_trashed || (timeline_only && !in_timeline) {
        return None;
    }
    Some(LibraryAsset {
        id: RemoteAssetId::new(item.id),
        original_file_name: non_empty(item.original_file_name),
        local_date_time: non_empty(item.local_date_time),
        file_created_at: non_empty(item.file_created_at),
        file_modified_at: non_empty(item.file_modified_at),
        file_size: item.exif_info.and_then(|exif| exif.file_size_in_byte),
        live_photo_video_id: non_empty(item.live_photo_video_id).map(RemoteAssetId::new),
    })
}
