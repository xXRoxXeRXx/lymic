mod album;
mod bulk_check;
mod client;
pub(crate) mod ids;
mod people;
mod places;
mod response;
mod search;
mod session;
#[cfg(test)]
mod test_support;
mod timeline;
mod trash;
mod upload;

pub(crate) use album::{Album, AlbumAssignmentResult, AlbumCatalogueItem};
pub(crate) use bulk_check::BulkCheckDuplicate;
pub(crate) use client::{AssetDetails, AssetResponse, ImmichClient};
pub(crate) use ids::{AlbumId, RemoteAssetId};
pub(crate) use people::PeoplePage;
pub(crate) use places::Place;
pub(crate) use search::{SearchQuery, SearchSupport, SearchTimeline};
pub(crate) use session::ImmichSession;
pub(crate) use timeline::{AlbumTimeline, PlaceTimeline, TimelineAsset, TimelineBucket};
pub(crate) use trash::TrashRestoreResult;
pub(crate) use upload::SidecarDirectoryCache;

mod map;
mod memory;
pub(crate) use map::MapMarker;
pub(crate) use memory::{MemoriesPage, Memory};
