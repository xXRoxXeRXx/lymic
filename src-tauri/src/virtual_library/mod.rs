//! The platform-neutral projection of the Immich Library behind the read-only Virtual Library
//! (ADR-0003, #266). It answers "what is in this directory" and "which Remote Asset is this
//! file" for a path in the Virtual Library tree, and hides every path rule. The Windows adapter
//! (Phase 2) only translates Cloud Files callbacks into these two calls.
//!
//! Paths are relative to the Virtual Library root, use `/` (a `\` is accepted too) and carry no
//! platform semantics, so the module behaves the same on every OS.

use crate::immich::{ImmichClient, LibraryAlbum, LibraryAsset, RemoteAssetId};
use chrono::{DateTime, Utc};

mod albums;
mod live_photo;
mod path;
mod photo_timeline;
#[cfg(test)]
mod tests;

use path::{Dir, Target};

/// What an [`Entry`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Directory,
    File,
}

/// One item of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The name shown in the directory, unique among its siblings.
    pub name: String,
    pub kind: EntryKind,
    /// The Remote Asset a file represents; `None` for directories.
    pub remote_asset: Option<RemoteAssetId>,
    /// The size of the original in bytes, when Immich reports one.
    pub size: Option<u64>,
    pub created: Option<DateTime<Utc>>,
    pub modified: Option<DateTime<Utc>>,
}

impl Entry {
    fn directory(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: EntryKind::Directory,
            remote_asset: None,
            size: None,
            created: None,
            modified: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VirtualLibraryError {
    /// No such directory or file in the Virtual Library.
    NotFound,
    /// `list` was asked for a file.
    NotADirectory,
    /// `resolve` was asked for a directory.
    NotAFile,
    /// Immich could not answer; the message never carries credentials.
    Remote(String),
}
/// The Virtual Library tree over one Immich Library.
pub(crate) struct VirtualLibrary<'a> {
    client: &'a ImmichClient,
}

impl<'a> VirtualLibrary<'a> {
    pub(crate) fn new(client: &'a ImmichClient) -> Self {
        Self { client }
    }

    /// The entries of the directory at `path` (`""` is the root).
    pub(crate) async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
        match path::parse(path).ok_or(VirtualLibraryError::NotFound)? {
            Target::Dir(dir) => self.children(dir).await,
            Target::File { dir, name } => {
                self.find_file(dir, &name).await?;
                Err(VirtualLibraryError::NotADirectory)
            }
        }
    }

    /// The Remote Asset the file at `path` represents.
    pub(crate) async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        match path::parse(path).ok_or(VirtualLibraryError::NotFound)? {
            Target::Dir(dir) => {
                self.children(dir).await?;
                Err(VirtualLibraryError::NotAFile)
            }
            Target::File { dir, name } => self
                .find_file(dir, &name)
                .await?
                .remote_asset
                .ok_or(VirtualLibraryError::NotFound),
        }
    }

    async fn find_file(&self, dir: Dir, name: &str) -> Result<Entry, VirtualLibraryError> {
        let wanted = name.to_lowercase();
        self.children(dir)
            .await?
            .into_iter()
            .find(|entry| entry.name.to_lowercase() == wanted)
            .ok_or(VirtualLibraryError::NotFound)
    }

    async fn library_albums(&self) -> Result<Vec<LibraryAlbum>, VirtualLibraryError> {
        self.client
            .list_library_albums()
            .await
            .map_err(VirtualLibraryError::Remote)
    }

    async fn children(&self, dir: Dir) -> Result<Vec<Entry>, VirtualLibraryError> {
        match dir {
            Dir::Root => Ok(vec![
                Entry::directory(path::ALL_PHOTOS),
                Entry::directory(path::ALBUMS),
            ]),
            Dir::Albums => {
                let albums = self.library_albums().await?;
                Ok(albums::directories(&albums))
            }
            Dir::Album(name) => {
                let albums = self.library_albums().await?;
                let album = albums::find(&albums, &name).ok_or(VirtualLibraryError::NotFound)?;
                let assets = self
                    .client
                    .list_album_assets(&album.id)
                    .await
                    .map_err(VirtualLibraryError::Remote)?;
                // An Album lists its hidden members too, so a Live Photo video shows up as an
                // asset of its own until the still claims it.
                let assets = live_photo::without_paired_videos(assets);
                let videos = self.client.list_live_photo_videos(&assets).await;
                Ok(albums::files(&assets, &videos))
            }
            Dir::AllPhotos => Ok(self.calendar().await?.years()),
            Dir::Year(year) => self
                .calendar()
                .await?
                .months(year)
                .ok_or(VirtualLibraryError::NotFound),
            Dir::Month(year, month) => {
                // Immich can only filter on the UTC instant of a capture, so the window is wider
                // than the month and the local month is applied to what comes back.
                let (after, before) = photo_timeline::creation_window(year, month);
                let assets = self
                    .client
                    .list_photo_timeline_assets_created_between(after, before)
                    .await
                    .map_err(VirtualLibraryError::Remote)?;
                self.timeline_files(photo_timeline::assets_in(assets, &dir))
                    .await
            }
            Dir::Undated => {
                // Immich cannot filter on a missing capture time: page through the timeline and
                // keep only the undated assets.
                let assets = self
                    .client
                    .find_photo_timeline_assets(&photo_timeline::is_undated)
                    .await
                    .map_err(VirtualLibraryError::Remote)?;
                self.timeline_files(assets).await
            }
        }
    }

    /// The months of the Photo Timeline, from Immich's timeline buckets: no asset is fetched.
    async fn calendar(&self) -> Result<photo_timeline::Calendar, VirtualLibraryError> {
        let buckets = self
            .client
            .list_photo_timeline_months()
            .await
            .map_err(VirtualLibraryError::Remote)?;
        Ok(photo_timeline::Calendar::from_buckets(&buckets))
    }

    /// The files of a month or `Undated` directory holding `assets`.
    async fn timeline_files(
        &self,
        assets: Vec<LibraryAsset>,
    ) -> Result<Vec<Entry>, VirtualLibraryError> {
        let assets = live_photo::without_paired_videos(assets);
        let videos = self.client.list_live_photo_videos(&assets).await;
        photo_timeline::files(&assets, &videos).ok_or(VirtualLibraryError::NotFound)
    }
}
