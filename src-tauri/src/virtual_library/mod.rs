//! The platform-neutral projection of the Immich Library behind the read-only Virtual Library
//! (ADR-0003, #266). It answers "what is in this directory" and "which Remote Asset is this
//! file" for a path in the Virtual Library tree, and hides every path rule. The Windows adapter
//! (Phase 2) only translates Cloud Files callbacks into these two calls.
//!
//! Paths are relative to the Virtual Library root, use `/` (a `\` is accepted too) and carry no
//! platform semantics, so the module behaves the same on every OS.

use crate::immich::{ImmichClient, LibraryAlbum, LibraryAsset, RemoteAssetId, RemoteFailure};
use chrono::{DateTime, Utc};

mod albums;
mod live_photo;
mod names;
mod ntfs;
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
    /// Immich could not answer; the failure says why and never carries credentials.
    Remote(RemoteFailure),
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

    /// Why `list` or `resolve` found nothing at `path`, for the diagnostic log: what Immich's
    /// timeline reports there against what the listing found. `None` when there is nothing to add
    /// to "not found" (a path that names nothing in the tree).
    pub(crate) async fn explain_missing(&self, path: &str) -> Option<String> {
        let (Target::Dir(dir) | Target::File { dir, .. }) = path::parse(path)?;
        let unreadable =
            |error: VirtualLibraryError| format!("the timeline could not be read: {error:?}");
        match dir {
            Dir::Year(year) => Some(match self.calendar().await {
                Ok(calendar) => format!(
                    "the timeline reports months {:?} in {year:04}",
                    calendar.months_of(year)
                ),
                Err(error) => unreadable(error),
            }),
            Dir::Month(year, month) => {
                let reported = match self.calendar().await {
                    Ok(calendar) => calendar.count_in(year, month),
                    Err(error) => return Some(unreadable(error)),
                };
                if reported == 0 {
                    return Some(format!(
                        "the timeline reports no asset in {year:04}-{month:02}"
                    ));
                }
                let (after, before) = photo_timeline::creation_window(year, month);
                let found = match self
                    .client
                    .list_photo_timeline_assets_created_between(after, before)
                    .await
                {
                    Ok(found) => found,
                    Err(failure) => {
                        return Some(format!(
                            "the timeline reports {reported} asset(s) in {year:04}-{month:02}; the search failed: {failure:?}"
                        ))
                    }
                };
                let found_total = found.len();
                let in_month = photo_timeline::assets_in(found, &dir).len();
                Some(format!(
                    "the timeline reports {reported} asset(s) in {year:04}-{month:02}; the search between {after} and {before} returned {found_total} asset(s), {in_month} of them in that local month"
                ))
            }
            Dir::Undated => Some(match self.calendar().await {
                Ok(calendar) => format!(
                    "the timeline reports {} asset(s) without a capture time",
                    calendar.undated_count()
                ),
                Err(error) => unreadable(error),
            }),
            Dir::Album(name) => Some(match self.library_albums().await {
                Ok(albums) => format!(
                    "no Album folder is named '{name}' among {} Album(s)",
                    albums.len()
                ),
                Err(error) => format!("the Albums could not be listed: {error:?}"),
            }),
            Dir::Root | Dir::AllPhotos | Dir::Albums => None,
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
                let videos = self
                    .client
                    .list_live_photo_videos(&assets)
                    .await
                    .map_err(VirtualLibraryError::Remote)?;
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
                let mut assets = photo_timeline::assets_in(assets, &dir);
                if assets.is_empty() && self.timeline_reports(year, month).await > 0 {
                    // The timeline holds assets of this local month that the window misses (their
                    // creation instant is not near their capture time): the one way to find them
                    // is the pass over the whole timeline, which an empty month makes worth it.
                    assets = self
                        .client
                        .find_photo_timeline_assets(&|asset| {
                            photo_timeline::is_filed_in(asset, &dir)
                        })
                        .await
                        .map_err(VirtualLibraryError::Remote)?;
                }
                self.timeline_files(assets).await
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

    /// How many assets Immich's timeline reports in the local month; 0 when it reports none or
    /// cannot be asked (the caller only uses it to look harder, never to show less).
    async fn timeline_reports(&self, year: i32, month: u32) -> u64 {
        match self.calendar().await {
            Ok(calendar) => calendar.count_in(year, month),
            Err(_) => 0,
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
        let videos = self
            .client
            .list_live_photo_videos(&assets)
            .await
            .map_err(VirtualLibraryError::Remote)?;
        photo_timeline::files(&assets, &videos).ok_or(VirtualLibraryError::NotFound)
    }
}
