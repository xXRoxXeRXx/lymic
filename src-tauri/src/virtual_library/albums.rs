//! `Albums`: one folder per Album, owned or shared with the user, holding what Immich returns
//! for that Album.

use super::{live_photo, Entry};
use crate::immich::{LibraryAlbum, LibraryAsset, LivePhotoVideo, RemoteAssetId};
use std::collections::HashMap;

/// The Album folders of `Albums`, ordered by name.
pub(super) fn directories(albums: &[LibraryAlbum]) -> Vec<Entry> {
    ordered(albums)
        .into_iter()
        .map(|album| Entry::directory(&album.album_name))
        .collect()
}

/// The Album whose folder is named `name`; names compare without regard to case.
pub(super) fn find<'a>(albums: &'a [LibraryAlbum], name: &str) -> Option<&'a LibraryAlbum> {
    let wanted = name.to_lowercase();
    ordered(albums)
        .into_iter()
        .find(|album| album.album_name.to_lowercase() == wanted)
}

/// The files of an Album folder for the Remote Assets Immich returned for it, a Live Photo as
/// its still and its video.
pub(super) fn files(
    assets: &[LibraryAsset],
    videos: &HashMap<RemoteAssetId, LivePhotoVideo>,
) -> Vec<Entry> {
    let mut files: Vec<Entry> = assets
        .iter()
        .flat_map(|asset| live_photo::file_entries(asset, videos))
        .collect();
    files.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.remote_asset.clone()));
    files
}

fn ordered(albums: &[LibraryAlbum]) -> Vec<&LibraryAlbum> {
    let mut ordered: Vec<&LibraryAlbum> = albums.iter().collect();
    ordered.sort_by_cached_key(|album| (album.album_name.to_lowercase(), album.id.to_string()));
    ordered
}
