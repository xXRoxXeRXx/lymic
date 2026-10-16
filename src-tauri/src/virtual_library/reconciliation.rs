use super::{
    albums::{AlbumProjection, RemoteAlbum},
    sync_root::CloudFilter,
    timeline::{RemoteVisualAsset, TimelineProjection},
};
use crate::db::{
    apply_virtual_library_delta, replace_virtual_library_snapshot,
    virtual_library_reconciliation_cursor, VirtualLibraryBinding,
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
    fn updated_visual_assets_page(&self, updated_after: Option<&str>, page: u32, page_size: usize) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
        let updated_after = updated_after.map(str::to_owned);
        Box::pin(async move {
            self.visual_assets_updated_after(updated_after.as_deref(), page, page_size)
                .await
        })
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

/// Runs one configured-library delta using the current credentials. This is
/// shared by startup, the ten-minute timer, and the user-initiated refresh.
pub(crate) async fn refresh_configured_library(
    pool: &SqlitePool,
    root: &crate::virtual_library::sync_root::SyncRootLifecycle,
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
        struct DeltaCatalog { seen: Mutex<Vec<Option<String>>>, assets: Vec<RemoteVisualAsset> }
        impl UpdatedVisualLibraryCatalog for DeltaCatalog {
            fn updated_visual_assets_page(&self, cursor: Option<&str>, page: u32, size: usize) -> BoxFuture<'_, Result<Vec<RemoteVisualAsset>, String>> {
                self.seen.lock().unwrap().push(cursor.map(str::to_owned));
                Box::pin(async move { Ok(self.assets.iter().skip((page as usize - 1) * size).take(size).cloned().collect()) })
            }
        }
        let db = crate::db::init_at_path(&std::env::temp_dir().join(format!("delta-{}.db", uuid::Uuid::new_v4()))).await.unwrap().pool;
        crate::db::replace_virtual_library_snapshot(&db, &[], "2026-01-01T00:00:00Z").await.unwrap();
        let catalog = DeltaCatalog { seen: Mutex::new(Vec::new()), assets: vec![asset(9)] };
        let filter = Arc::new(Filter::default());
        assert_eq!(reconcile_library_delta(&db, "C:\\Library", &catalog, filter.clone(), "2026-01-02T00:00:00Z").await.unwrap(), 1);
        assert_eq!(*catalog.seen.lock().unwrap(), vec![Some("2026-01-01T00:00:00Z".into())]);
        assert_eq!(crate::db::virtual_library_reconciliation_cursor(&db).await.unwrap(), Some("2026-01-02T00:00:00Z".into()));
        assert_eq!(filter.paths.lock().unwrap().len(), 1);
    }
}
