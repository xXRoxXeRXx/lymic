use super::{
    albums::{AlbumProjection, RemoteAlbum},
    sync_root::CloudFilter,
    timeline::{RemoteVisualAsset, TimelineProjection},
};
use crate::db::{
    apply_virtual_library_delta, remove_virtual_library_bindings, replace_virtual_library_snapshot,
    virtual_library_bindings, virtual_library_reconciliation_cursor, VirtualLibraryBinding,
};
use futures::future::BoxFuture;
use sqlx::SqlitePool;
use std::{collections::HashSet, sync::Arc};

pub(crate) const RECONCILIATION_PAGE_SIZE: usize = 100;

/// Boundary for the paginated Immich visual-library APIs. Keeping it separate
/// makes incomplete pages and interrupted reconciliation independently testable.
pub(crate) trait VisualLibraryCatalog: Send + Sync {
    fn visual_assets_page(
        &self,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>>;
    fn albums_page(
        &self,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteAlbum>, String>>;
}

/// The narrow remote boundary used by periodic synchronization. `updated_after`
/// is the durable cursor from the previous successful run.
pub(crate) trait UpdatedVisualLibraryCatalog: Send + Sync {
    fn updated_visual_assets_page(
        &self,
        updated_after: Option<&str>,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>>;
}

impl UpdatedVisualLibraryCatalog for crate::immich::ImmichClient {
    fn updated_visual_assets_page(
        &self,
        updated_after: Option<&str>,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
        let updated_after = updated_after.map(str::to_owned);
        Box::pin(async move {
            self.visual_assets_updated_after(updated_after.as_deref(), page, page_size)
                .await
        })
    }
}

impl VisualLibraryCatalog for crate::immich::ImmichClient {
    fn visual_assets_page(
        &self,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
        Box::pin(async move {
            self.visual_assets_updated_after(None, page, page_size)
                .await
        })
    }
    fn albums_page(
        &self,
        page: u32,
        page_size: usize,
    ) -> BoxFuture<'_, Result<Vec<RemoteAlbum>, String>> {
        Box::pin(async move { self.virtual_library_albums_page(page, page_size).await })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReconciliationProgress {
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) percent: u8,
}

/// Projects just the assets changed since the last committed cursor. The
/// cursor advances only after all placeholders and their durable bindings were
/// written, making the operation safe to retry after interruption.
pub(crate) async fn reconcile_library_delta(
    pool: &SqlitePool,
    root: &str,
    catalog: &dyn UpdatedVisualLibraryCatalog,
    filter: Arc<dyn CloudFilter>,
    cursor: &str,
) -> Result<usize, String> {
    let updated_after = virtual_library_reconciliation_cursor(pool)
        .await
        .map_err(|error| error.to_string())?;
    let assets = all_pages(|page| {
        catalog.updated_visual_assets_page(updated_after.as_deref(), page, RECONCILIATION_PAGE_SIZE)
    })
    .await?;
    let timeline = TimelineProjection::new(filter);
    let mut bindings = Vec::new();
    for asset in &assets {
        for projection_path in timeline.project(root, asset)? {
            bindings.push(VirtualLibraryBinding {
                asset_id: asset.id.clone(),
                projection_path,
                version: asset.version.clone(),
            });
        }
    }
    apply_virtual_library_delta(pool, &bindings, cursor)
        .await
        .map_err(|error| error.to_string())?;
    Ok(assets.len())
}

/// Prunes projections for Remote Assets absent from an authoritative library
/// snapshot. This intentionally does not use `updatedAfter`: Immich's update
/// stream does not promise tombstones for hard-deleted assets.
///
/// A locked placeholder is left bound and is retried on the next snapshot.
/// This is the durable deferred-pruning queue; it avoids deleting a binding
/// before its Explorer-visible placeholder has actually gone away.
pub(crate) async fn prune_remote_deleted_assets(
    pool: &SqlitePool,
    current_asset_ids: &HashSet<String>,
    filter: Arc<dyn CloudFilter>,
    thumbnails: &crate::thumbnail_cache::ThumbnailCache,
) -> Result<usize, String> {
    let stale = virtual_library_bindings(pool)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|binding| !current_asset_ids.contains(&binding.asset_id))
        .collect::<Vec<_>>();
    let mut removed = Vec::new();
    let mut fully_removed_assets = HashSet::new();
    for binding in &stale {
        if filter.remove_placeholder(&binding.projection_path).is_ok() {
            removed.push(binding.clone());
            fully_removed_assets.insert(binding.asset_id.clone());
        }
    }
    // Do not evict a thumbnail if any projection for that Remote Asset is still
    // locked: the retained binding makes the retry state explicit and avoids a
    // transient cache miss while Explorer still displays the file.
    for binding in &stale {
        if !removed.iter().any(|removed| removed == binding) {
            fully_removed_assets.remove(&binding.asset_id);
        }
    }
    remove_virtual_library_bindings(pool, &removed)
        .await
        .map_err(|error| error.to_string())?;
    for asset_id in &fully_removed_assets {
        thumbnails.remove_asset(asset_id)?;
    }
    Ok(removed.len())
}

/// Reconciles the authoritative Remote Asset set before pruning. A complete
/// enumeration is required here: `updatedAfter` reports changes to extant
/// assets, but cannot establish that an omitted asset was hard-deleted.
async fn reconcile_authoritative_assets(
    pool: &SqlitePool,
    root: &str,
    catalog: &dyn UpdatedVisualLibraryCatalog,
    filter: Arc<dyn CloudFilter>,
    cursor: &str,
    thumbnails: &crate::thumbnail_cache::ThumbnailCache,
) -> Result<usize, String> {
    let assets =
        all_pages(|page| catalog.updated_visual_assets_page(None, page, RECONCILIATION_PAGE_SIZE))
            .await?;
    let timeline = TimelineProjection::new(filter.clone());
    let mut bindings = Vec::new();
    for asset in &assets {
        for projection_path in timeline.project(root, asset)? {
            bindings.push(VirtualLibraryBinding {
                asset_id: asset.id.clone(),
                projection_path,
                version: asset.version.clone(),
            });
        }
    }
    apply_virtual_library_delta(pool, &bindings, cursor)
        .await
        .map_err(|error| error.to_string())?;
    let current_asset_ids = assets.into_iter().map(|asset| asset.id).collect();
    prune_remote_deleted_assets(pool, &current_asset_ids, filter, thumbnails).await
}

/// Runs one configured-library delta using the current credentials. This is
/// shared by startup, the ten-minute timer, and the user-initiated refresh.
/// Full enumeration remains available only for explicit deletion detection;
/// normal refreshes must keep using Immich's cursor.
pub(crate) async fn refresh_configured_library(
    pool: &SqlitePool,
    root: &crate::virtual_library::sync_root::SyncRootLifecycle,
    _app: &tauri::AppHandle,
) -> Result<usize, String> {
    let status = crate::db::get_virtual_library_status(pool)
        .await
        .map_err(|error| error.to_string())?;
    if !status.enabled {
        return Err("Virtual Library is not enabled.".to_string());
    }
    let credentials = crate::auth::get_credentials()?.ok_or("Not logged in")?;
    let client = crate::immich::ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )?;
    let cursor = virtual_library_reconciliation_cursor(pool)
        .await
        .map_err(|error| error.to_string())?;
    let is_missing_projections = !std::path::Path::new(&status.path)
        .join("All Photos")
        .exists();
    if cursor.is_none() || is_missing_projections {
        reconcile_initial_library(
            pool,
            &status.path,
            &client,
            root.projection_filter(),
            |_| {},
        )
        .await?;
        return Ok(0);
    }
    reconcile_library_delta(
        pool,
        &status.path,
        &client,
        root.projection_filter(),
        &chrono::Utc::now().to_rfc3339(),
    )
    .await
}

/// Enumerates all pages before publishing a durable snapshot. A failure leaves
/// the previous watermark intact; a retry can therefore replay safely.
pub(crate) async fn reconcile_initial_library(
    pool: &SqlitePool,
    root: &str,
    catalog: &dyn VisualLibraryCatalog,
    filter: Arc<dyn CloudFilter>,
    mut report: impl FnMut(ReconciliationProgress),
) -> Result<(), String> {
    let assets =
        all_pages(|page| catalog.visual_assets_page(page, RECONCILIATION_PAGE_SIZE)).await?;
    let albums = all_pages(|page| catalog.albums_page(page, RECONCILIATION_PAGE_SIZE)).await?;
    let total = assets.len() + albums.iter().map(|album| album.assets.len()).sum::<usize>();
    let timeline = TimelineProjection::new(filter.clone());
    let album_projection = AlbumProjection::new(filter);
    let mut bindings = Vec::new();
    let mut paths = HashSet::new();
    let mut completed = 0;

    for chunk in assets.chunks(RECONCILIATION_PAGE_SIZE) {
        for asset in chunk {
            for path in timeline.project(root, asset)? {
                if paths.insert(path.clone()) {
                    bindings.push(VirtualLibraryBinding {
                        asset_id: asset.id.clone(),
                        projection_path: path,
                        version: asset.version.clone(),
                    });
                }
            }
            completed += 1;
        }
        report(progress(completed, total));
    }
    for chunk in albums.chunks(RECONCILIATION_PAGE_SIZE) {
        for album in chunk {
            for path in album_projection.project(root, album)? {
                if paths.insert(path.clone()) {
                    // AlbumProjection currently exposes the album asset paths in asset order.
                    let asset = album
                        .assets
                        .iter()
                        .find(|asset| path.ends_with(&asset.file_name))
                        .or_else(|| album.assets.first());
                    if let Some(asset) = asset {
                        bindings.push(VirtualLibraryBinding {
                            asset_id: asset.id.clone(),
                            projection_path: path,
                            version: asset.version.clone(),
                        });
                    }
                }
            }
            completed += album.assets.len();
        }
        report(progress(completed, total));
    }
    replace_virtual_library_snapshot(pool, &bindings, &chrono::Utc::now().to_rfc3339())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn progress(completed: usize, total: usize) -> ReconciliationProgress {
    ReconciliationProgress {
        completed,
        total,
        percent: if total == 0 {
            100
        } else {
            ((completed * 100) / total).min(100) as u8
        },
    }
}

async fn all_pages<'a, T>(
    mut fetch: impl FnMut(u32) -> BoxFuture<'a, Result<Vec<T>, String>>,
) -> Result<Vec<T>, String> {
    let mut page = 1;
    let mut all = Vec::new();
    loop {
        let entries = fetch(page).await?;
        let done = entries.len() < RECONCILIATION_PAGE_SIZE;
        all.extend(entries);
        if done {
            return Ok(all);
        }
        page += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Filter {
        paths: Mutex<Vec<String>>,
        removed: Mutex<Vec<String>>,
        locked: Mutex<HashSet<String>>,
    }
    impl CloudFilter for Filter {
        fn register(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn connect(&self, _: &str) -> Result<i64, String> {
            Ok(1)
        }
        fn disconnect(&self, _: i64) -> Result<(), String> {
            Ok(())
        }
        fn unregister(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_directory(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_placeholder(
            &self,
            path: &str,
            _: super::super::sync_root::PlaceholderMetadata,
        ) -> Result<(), String> {
            self.paths.lock().unwrap().push(path.into());
            Ok(())
        }
        fn remove_placeholder(&self, path: &str) -> Result<(), String> {
            if self.locked.lock().unwrap().contains(path) {
                return Err("placeholder is locked".into());
            }
            self.removed.lock().unwrap().push(path.into());
            Ok(())
        }
        fn acknowledge_change(
            &self,
            _: super::super::sync_root::ChangeNotification,
            _: super::super::sync_root::ChangeRequest,
            _: i32,
        ) -> Result<(), String> {
            Ok(())
        }
    }
    struct Catalog {
        assets: Vec<RemoteVisualAsset>,
        albums: Vec<RemoteAlbum>,
    }
    impl UpdatedVisualLibraryCatalog for Catalog {
        fn updated_visual_assets_page(
            &self,
            _: Option<&str>,
            page: u32,
            size: usize,
        ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
            self.visual_assets_page(page, size)
        }
    }
    impl VisualLibraryCatalog for Catalog {
        fn visual_assets_page(
            &self,
            page: u32,
            size: usize,
        ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
            Box::pin(async move {
                Ok(self
                    .assets
                    .iter()
                    .skip((page as usize - 1) * size)
                    .take(size)
                    .cloned()
                    .collect())
            })
        }
        fn albums_page(
            &self,
            page: u32,
            size: usize,
        ) -> BoxFuture<'_, Result<Vec<RemoteAlbum>, String>> {
            Box::pin(async move {
                Ok(self
                    .albums
                    .iter()
                    .skip((page as usize - 1) * size)
                    .take(size)
                    .cloned()
                    .collect())
            })
        }
    }
    fn asset(id: usize) -> RemoteVisualAsset {
        RemoteVisualAsset {
            id: format!("asset-{id}"),
            version: "v1".into(),
            file_name: format!("{id}.jpg"),
            file_size: 1,
            captured_at: Some(Utc::now()),
            live_photo_video: None,
        }
    }
    #[tokio::test]
    async fn enumerates_multiple_pages_projects_in_batches_and_replaces_the_durable_snapshot() {
        let db = crate::db::init_at_path(
            &std::env::temp_dir().join(format!("reconcile-{}.db", uuid::Uuid::new_v4())),
        )
        .await
        .unwrap()
        .pool;
        let assets = (0..101).map(asset).collect::<Vec<_>>();
        let catalog = Catalog {
            albums: vec![RemoteAlbum {
                id: "album".into(),
                name: "Trip".into(),
                assets: vec![assets[0].clone()],
            }],
            assets,
        };
        let filter = Arc::new(Filter::default());
        let updates = Mutex::new(Vec::new());
        reconcile_initial_library(&db, "C:\\Library", &catalog, filter.clone(), |p| {
            updates.lock().unwrap().push(p.percent)
        })
        .await
        .unwrap();
        assert_eq!(
            crate::db::virtual_library_bindings(&db)
                .await
                .unwrap()
                .len(),
            102
        );
        assert_eq!(filter.paths.lock().unwrap().len(), 102);
        assert_eq!(updates.lock().unwrap().last(), Some(&100));
    }

    #[tokio::test]
    async fn delta_queries_from_the_stored_cursor_and_advances_it_after_projecting_new_assets() {
        struct DeltaCatalog {
            seen: Mutex<Vec<Option<String>>>,
            assets: Vec<RemoteVisualAsset>,
        }
        impl UpdatedVisualLibraryCatalog for DeltaCatalog {
            fn updated_visual_assets_page(
                &self,
                cursor: Option<&str>,
                page: u32,
                size: usize,
            ) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
                self.seen.lock().unwrap().push(cursor.map(str::to_owned));
                Box::pin(async move {
                    Ok(self
                        .assets
                        .iter()
                        .skip((page as usize - 1) * size)
                        .take(size)
                        .cloned()
                        .collect())
                })
            }
        }
        let db = crate::db::init_at_path(
            &std::env::temp_dir().join(format!("delta-{}.db", uuid::Uuid::new_v4())),
        )
        .await
        .unwrap()
        .pool;
        crate::db::replace_virtual_library_snapshot(&db, &[], "2026-01-01T00:00:00Z")
            .await
            .unwrap();
        let catalog = DeltaCatalog {
            seen: Mutex::new(Vec::new()),
            assets: vec![asset(9)],
        };
        let filter = Arc::new(Filter::default());
        assert_eq!(
            reconcile_library_delta(
                &db,
                "C:\\Library",
                &catalog,
                filter.clone(),
                "2026-01-02T00:00:00Z"
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            *catalog.seen.lock().unwrap(),
            vec![Some("2026-01-01T00:00:00Z".into())]
        );
        assert_eq!(
            crate::db::virtual_library_reconciliation_cursor(&db)
                .await
                .unwrap(),
            Some("2026-01-02T00:00:00Z".into())
        );
        assert_eq!(filter.paths.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn prunes_every_projection_and_thumbnail_for_a_deleted_remote_asset_but_retries_locked_files(
    ) {
        let db = crate::db::init_at_path(
            &std::env::temp_dir().join(format!("prune-{}.db", uuid::Uuid::new_v4())),
        )
        .await
        .unwrap()
        .pool;
        let deleted_timeline = "C:\\Library\\All Photos\\Unknown\\deleted.jpg";
        let deleted_album = "C:\\Library\\Albums\\Trip\\deleted.jpg";
        let locked = "C:\\Library\\Albums\\Trip\\locked.jpg";
        crate::db::replace_virtual_library_snapshot(
            &db,
            &[
                VirtualLibraryBinding {
                    asset_id: "deleted".into(),
                    projection_path: deleted_timeline.into(),
                    version: "v1".into(),
                },
                VirtualLibraryBinding {
                    asset_id: "deleted".into(),
                    projection_path: deleted_album.into(),
                    version: "v1".into(),
                },
                VirtualLibraryBinding {
                    asset_id: "locked".into(),
                    projection_path: locked.into(),
                    version: "v1".into(),
                },
                VirtualLibraryBinding {
                    asset_id: "retained".into(),
                    projection_path: "C:\\Library\\All Photos\\Unknown\\retained.jpg".into(),
                    version: "v1".into(),
                },
            ],
            "2026-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        let filter = Arc::new(Filter::default());
        filter.locked.lock().unwrap().insert(locked.into());
        let cache_root =
            std::env::temp_dir().join(format!("prune-thumbnails-{}", uuid::Uuid::new_v4()));
        let cache = crate::thumbnail_cache::ThumbnailCache::new(cache_root.clone(), 1_000_000);
        let thumbnail = cache
            .store("deleted", "v1", "image/jpeg", b"thumbnail")
            .unwrap();

        let current = HashSet::from(["retained".to_string()]);
        assert_eq!(
            prune_remote_deleted_assets(&db, &current, filter.clone(), &cache)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            *filter.removed.lock().unwrap(),
            [deleted_album.to_string(), deleted_timeline.to_string()]
        );
        assert!(!thumbnail.exists());
        assert_eq!(
            crate::db::virtual_library_bindings(&db).await.unwrap(),
            vec![
                VirtualLibraryBinding {
                    asset_id: "locked".into(),
                    projection_path: locked.into(),
                    version: "v1".into()
                },
                VirtualLibraryBinding {
                    asset_id: "retained".into(),
                    projection_path: "C:\\Library\\All Photos\\Unknown\\retained.jpg".into(),
                    version: "v1".into()
                },
            ]
        );
        std::fs::remove_dir_all(cache_root).unwrap();
    }
}
