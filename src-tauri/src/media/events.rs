use super::extensions::{is_media_file, live_photo_sibling_paths};
use super::model::{Asset, FileFailure, ScanResult};
use super::scanner::{scan_folders_for_media, scan_paths_for_media};
use crate::app::events::log_to_ui;
use std::collections::HashMap;

pub(crate) async fn add_event_paths(
    files_buffer: &mut HashMap<String, Asset>,
    failures_buffer: &mut HashMap<String, FileFailure>,
    event: notify::Event,
) -> Result<(), String> {
    // Determining whether an event path is a directory can access slow or offline storage.
    let (directories, files) = tokio::task::spawn_blocking(move || {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for path in event.paths {
            if path.is_dir() {
                directories.push(path);
            } else if is_media_file(&path) {
                files.extend(live_photo_sibling_paths(&path));
                files.push(path);
            }
        }
        (directories, files)
    })
    .await
    .map_err(|error| format!("Could not classify watcher paths: {}", error))?;
    if directories.is_empty() && files.is_empty() {
        return Ok(());
    }
    let (folder_scan, file_scan) = tokio::try_join!(
        scan_folders_for_media(directories),
        scan_paths_for_media(files)
    )?;
    for scan_result in [folder_scan, file_scan] {
        for asset in scan_result.files {
            files_buffer.insert(asset.path.clone(), asset);
        }
        for failure in scan_result.failures {
            failures_buffer.insert(failure.path.clone(), failure);
        }
    }
    Ok(())
}

pub(crate) fn log_overlapping_folders(app: &tauri::AppHandle, scan_result: &ScanResult) {
    for (folder, parent) in &scan_result.overlapping_folders {
        log_to_ui(
            app,
            "WARN",
            &format!(
                "Skipping overlapping watched folder '{}' because '{}' is already scanned.",
                folder.display(),
                parent.display()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::extensions::live_photo_sibling_paths;

    #[test]
    fn finds_existing_live_photo_sibling_for_watcher_path() {
        let root = std::env::temp_dir().join(format!("lymic-events-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let image = root.join("IMG_1.HEIC");
        let video = root.join("IMG_1.Mov");
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&video, b"video").unwrap();
        assert_eq!(live_photo_sibling_paths(&image), vec![video]);
        assert!(live_photo_sibling_paths(&root.join("other.jpg")).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
