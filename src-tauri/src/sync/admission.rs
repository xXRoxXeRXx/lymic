//! Converts scanned media results into durable queue Upload Units.

use crate::db;

/// Queue a producer result while copying its Watched Folder's current Target
/// Album into each Upload Unit. Kept free of Tauri so the durable admission
/// contract is directly testable.
pub(crate) async fn admit_scan_result(
    pool: &sqlx::SqlitePool,
    scan_result: &crate::media::model::ScanResult,
) -> Result<db::SyncSnapshot, String> {
    let folders = db::get_folders(pool)
        .await
        .map_err(|error| error.to_string())?;
    let (units, failures) = prepare_scan_for_queue(scan_result, &folders);
    db::enqueue_upload_units(pool, &units, &failures)
        .await
        .map_err(|error| format!("Could not persist sync queue: {error}"))
}

fn prepare_scan_for_queue(
    scan_result: &crate::media::model::ScanResult,
    folders: &[db::WatchedFolder],
) -> (Vec<db::QueueUploadUnit>, Vec<(String, String)>) {
    let folders = folders
        .iter()
        .map(|folder| {
            (
                crate::media::paths::normalize_path_lexical(std::path::Path::new(&folder.path)),
                folder.target_album_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let assets = scan_result
        .files
        .iter()
        .map(|asset| {
            let mut asset = asset.clone();
            let asset_path =
                crate::media::paths::normalize_path_lexical(std::path::Path::new(&asset.path));
            asset.target_album_id = asset.target_album_id.or_else(|| {
                folders.iter().find_map(|(folder_path, target_album_id)| {
                    if crate::media::paths::path_is_within_lexical(&asset_path, folder_path) {
                        target_album_id.clone()
                    } else {
                        None
                    }
                })
            });
            asset
        })
        .collect::<Vec<_>>();
    let units = crate::media::model::upload_units_from_assets(assets)
        .into_iter()
        .map(|unit| db::QueueUploadUnit {
            image: queue_upload_asset(unit.image),
            live_photo_video: unit.live_photo_video.map(queue_upload_asset),
        })
        .collect();

    let failures = scan_result
        .failures
        .iter()
        .map(|failure| (failure.path.clone(), failure.error.clone()))
        .collect();

    (units, failures)
}

fn queue_upload_asset(asset: crate::media::model::Asset) -> db::QueueUploadAsset {
    db::QueueUploadAsset {
        path: asset.path,
        size: asset.size as i64,
        mtime: asset.mtime,
        target_album_id: asset.target_album_id,
    }
}
