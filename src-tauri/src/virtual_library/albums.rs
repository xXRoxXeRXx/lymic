//! `Albums`: one folder per Album, owned or shared with the user, holding what Immich returns
//! for that Album.

use super::names::{self, Candidate, Shape};
use super::{live_photo, ntfs, Entry};
use crate::immich::{LibraryAlbum, LibraryAsset, LivePhotoVideo, RemoteAssetId};
use std::collections::HashMap;

/// The Album folders of `Albums`, ordered by name.
pub(super) fn directories(albums: &[LibraryAlbum]) -> Vec<Entry> {
    named(albums)
        .into_iter()
        .map(|(name, _)| Entry::directory(name))
        .collect()
}

/// The Album whose folder is named `name`; names compare without regard to case.
pub(super) fn find<'a>(albums: &'a [LibraryAlbum], name: &str) -> Option<&'a LibraryAlbum> {
    let wanted = name.to_lowercase();
    named(albums)
        .into_iter()
        .find(|(folder, _)| folder.to_lowercase() == wanted)
        .map(|(_, album)| album)
}

/// The files of an Album folder for the Remote Assets Immich returned for it, a Live Photo as
/// its still and its video.
pub(super) fn files(
    assets: &[LibraryAsset],
    videos: &HashMap<RemoteAssetId, LivePhotoVideo>,
) -> Vec<Entry> {
    live_photo::files(assets, videos)
}

/// The Albums with the unique folder name each one is listed under, ordered by that name.
fn named(albums: &[LibraryAlbum]) -> Vec<(String, &LibraryAlbum)> {
    let candidates: Vec<Candidate> = albums
        .iter()
        .map(|album| Candidate {
            name: ntfs::sanitize(&album.album_name),
            id: album.id.to_string(),
        })
        .collect();
    let mut named: Vec<(String, &LibraryAlbum)> = names::unique(&candidates, Shape::Directory)
        .into_iter()
        .zip(albums)
        .collect();
    named.sort_by_cached_key(|(name, album)| (name.to_lowercase(), album.id.to_string()));
    named
}
