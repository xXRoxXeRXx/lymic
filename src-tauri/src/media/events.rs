use super::extensions::{is_media_file, live_photo_sibling_paths};
use super::model::{Asset, FileFailure, ScanResult};
use super::scanner::{
    matches_exclusion_pattern, scan_paths_for_media, scan_watched_folders_for_media,
};
use crate::app::events::log_to_ui_keyed;
use crate::db::WatchedFolder;
use std::collections::HashMap;
use std::path::Path;

fn watched_folder_for_path<'a>(
    folders: &'a [WatchedFolder],
    path: &Path,
) -> Option<&'a WatchedFolder> {
    folders
        .iter()
        .filter(|folder| path.strip_prefix(&folder.path).is_ok())
        .max_by_key(|folder| Path::new(&folder.path).components().count())
}

fn is_excluded_by_watched_folder(folders: &[WatchedFolder], path: &Path) -> bool {
    watched_folder_for_path(folders, path).is_some_and(|folder| {
        path.strip_prefix(&folder.path).is_ok_and(|relative_path| {
            folder
                .exclusion_patterns
                .iter()
                .any(|pattern| matches_exclusion_pattern(pattern, relative_path))
        })
    })
}

fn belongs_to_excluded_live_photo_unit(folders: &[WatchedFolder], path: &Path) -> bool {
    is_excluded_by_watched_folder(folders, path)
        || live_photo_sibling_paths(path)
            .into_iter()
            .any(|sibling| is_excluded_by_watched_folder(folders, &sibling))
}

fn remove_excluded_assets(scan_result: &mut ScanResult, folders: &[WatchedFolder]) {
    scan_result
        .files
        .retain(|asset| !belongs_to_excluded_live_photo_unit(folders, Path::new(&asset.path)));
}

pub(crate) async fn add_event_paths(
    files_buffer: &mut HashMap<String, Asset>,
    failures_buffer: &mut HashMap<String, FileFailure>,
    folders: &[WatchedFolder],
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
    let mut folder_ids = std::collections::HashSet::new();
    let scanned_folders = directories
        .iter()
        .filter_map(|path| watched_folder_for_path(folders, path))
        .filter(|folder| folder_ids.insert(folder.id))
        .cloned()
        .collect();
    let (mut folder_scan, mut file_scan) = tokio::try_join!(
        scan_watched_folders_for_media(scanned_folders),
        scan_paths_for_media(files)
    )?;
    remove_excluded_assets(&mut folder_scan, folders);
    remove_excluded_assets(&mut file_scan, folders);
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
        let mut params = std::collections::HashMap::new();
        params.insert("folder".to_string(), folder.display().to_string());
        params.insert("parent".to_string(), parent.display().to_string());
        log_to_ui_keyed(
            app,
            "WARN",
            &format!(
                "Skipping overlapping watched folder '{}' because '{}' is already scanned.",
                folder.display(),
                parent.display()
            ),
            "log_skipping_overlapping_folder",
            params,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::extensions::live_photo_sibling_paths;
    use super::*;
    use crate::db::WatchedFolder;
    use notify::event::CreateKind;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn watched_folder(path: &std::path::Path, patterns: &[&str]) -> WatchedFolder {
        WatchedFolder {
            id: 1,
            path: path.to_string_lossy().to_string(),
            recursive: true,
            target_album_id: None,
            exclusion_patterns: patterns
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
        }
    }

    fn create_event(paths: Vec<PathBuf>) -> notify::Event {
        notify::Event {
            kind: notify::EventKind::Create(CreateKind::Any),
            paths,
            attrs: Default::default(),
        }
    }

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

    #[tokio::test]
    async fn watcher_events_apply_nested_case_insensitive_directory_and_extension_exclusions() {
        let root =
            std::env::temp_dir().join(format!("lymic-events-exclusions-{}", uuid::Uuid::new_v4()));
        let nested = root.join("Screenshots").join("Nested");
        std::fs::create_dir_all(&nested).unwrap();
        let excluded_by_directory = nested.join("one.JPG");
        let excluded_by_extension = root.join("two.MOV");
        let allowed = root.join("Trips").join("three.jpg");
        std::fs::create_dir_all(allowed.parent().unwrap()).unwrap();
        for path in [&excluded_by_directory, &excluded_by_extension, &allowed] {
            std::fs::write(path, b"image").unwrap();
        }

        let mut files = HashMap::new();
        let mut failures = HashMap::new();
        add_event_paths(
            &mut files,
            &mut failures,
            &[watched_folder(&root, &["screenshots/**", "**/*.mov"])],
            create_event(vec![root.clone()]),
        )
        .await
        .unwrap();

        assert_eq!(files.len(), 1);
        assert!(files.contains_key(&allowed.to_string_lossy().to_string()));
        assert!(failures.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn watcher_event_excluding_live_photo_component_suppresses_the_complete_upload_unit() {
        let root =
            std::env::temp_dir().join(format!("lymic-events-live-photo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let image = root.join("IMG_1.HEIC");
        let video = root.join("IMG_1.MOV");
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&video, b"video").unwrap();

        let mut files = HashMap::new();
        let mut failures = HashMap::new();
        add_event_paths(
            &mut files,
            &mut failures,
            &[watched_folder(&root, &["**/*.mov"])],
            create_event(vec![image]),
        )
        .await
        .unwrap();

        assert!(files.is_empty());
        assert!(failures.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
