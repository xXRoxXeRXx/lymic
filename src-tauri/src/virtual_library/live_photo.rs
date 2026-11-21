//! Live Photo pairs: a Live Photo is projected as two files of one directory, the still and its
//! video, sharing a base name (`IMG_0001.HEIC` and `IMG_0001.MOV`). The video is a separate
//! asset in Immich; it never shows up as a file of its own.
//!
//! These steps work on any list of [`LibraryAsset`]s, so every folder that projects assets
//! (the date hierarchy, Album folders) applies them the same way:
//! 1. [`without_paired_videos`] drops a video asset a still of the same list already claims;
//! 2. the caller looks the videos up with `ImmichClient::list_live_photo_videos`;
//! 3. [`file_entries`] turns each asset into its file entries: one, or two for a Live Photo.

use super::{photo_timeline, Entry};
use crate::immich::{LibraryAsset, LivePhotoVideo, RemoteAssetId};
use std::collections::{HashMap, HashSet};

/// The extension given to a video whose original file name has none.
const FALLBACK_VIDEO_EXTENSION: &str = "mov";

/// `assets` without the videos that a Live Photo still of the same list pairs with. A listing
/// that filters on visibility never contains them; one that does not (an Album lists hidden
/// members) would otherwise show the video a second time.
pub(super) fn without_paired_videos(assets: Vec<LibraryAsset>) -> Vec<LibraryAsset> {
    let paired: HashSet<RemoteAssetId> = assets
        .iter()
        .filter_map(|asset| asset.live_photo_video_id.clone())
        .collect();
    assets
        .into_iter()
        .filter(|asset| !paired.contains(&asset.id))
        .collect()
}

/// The file entries of `asset`: the still, followed by its video when `videos` knows it.
pub(super) fn file_entries(
    asset: &LibraryAsset,
    videos: &HashMap<RemoteAssetId, LivePhotoVideo>,
) -> Vec<Entry> {
    let still = photo_timeline::file_entry(asset);
    let Some((video_id, video)) = asset
        .live_photo_video_id
        .as_ref()
        .and_then(|id| Some((id, videos.get(id)?)))
    else {
        return vec![still];
    };
    let name = format!(
        "{}.{}",
        base_name(&still.name),
        extension(video.original_file_name.as_deref()).unwrap_or(FALLBACK_VIDEO_EXTENSION)
    );
    let video = Entry {
        name,
        remote_asset: Some(video_id.clone()),
        size: video.file_size,
        ..still.clone()
    };
    vec![still, video]
}

/// The name without its last extension (`IMG_0001.HEIC` is `IMG_0001`).
fn base_name(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((base, _)) if !base.is_empty() => base,
        _ => name,
    }
}

fn extension(name: Option<&str>) -> Option<&str> {
    let (base, extension) = name?.rsplit_once('.')?;
    (!base.is_empty() && !extension.is_empty()).then_some(extension)
}
