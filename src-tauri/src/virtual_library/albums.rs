use super::{
    filenames::FilenameResolver,
    sync_root::{CloudFilter, PlaceholderMetadata},
    timeline::RemoteVisualAsset,
};
use std::collections::HashMap;
use std::sync::Arc;

/// An Immich album and the assets it exposes in the Virtual Library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteAlbum {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) assets: Vec<RemoteVisualAsset>,
}

/// Projects the Album view independently from the canonical All Photos view.
pub(crate) struct AlbumProjection {
    adapter: Arc<dyn CloudFilter>,
    album_directories: std::sync::Mutex<FilenameResolver>,
    filenames_by_directory: std::sync::Mutex<HashMap<String, FilenameResolver>>,
}

impl AlbumProjection {
    pub(crate) fn new(adapter: Arc<dyn CloudFilter>) -> Self {
        Self {
            adapter,
            album_directories: std::sync::Mutex::new(FilenameResolver::new()),
            filenames_by_directory: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn project(&self, root: &str, album: &RemoteAlbum) -> Result<(), String> {
        let albums = format!("{root}\\Albums");
        self.adapter.create_directory(&albums)?;
        let album_directory = self
            .album_directories
            .lock()
            .map_err(|error| error.to_string())?
            .resolve(&album.name, &album.id);
        let directory = format!("{albums}\\{album_directory}");
        self.adapter.create_directory(&directory)?;

        let mut resolvers = self
            .filenames_by_directory
            .lock()
            .map_err(|error| error.to_string())?;
        let resolver = resolvers
            .entry(directory.clone())
            .or_insert_with(FilenameResolver::new);
        for asset in &album.assets {
            let file_name = resolver.resolve(&asset.file_name, &asset.id);
            self.adapter.create_placeholder(
                &format!("{directory}\\{file_name}"),
                PlaceholderMetadata {
                    file_size: asset.file_size,
                    captured_at: asset.captured_at,
                    asset_id: asset.id.clone(),
                    version: asset.version.clone(),
                },
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_library::timeline::TimelineProjection;
    use chrono::{TimeZone, Utc};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeCloudFilter {
        directories: Mutex<Vec<String>>,
        placeholders: Mutex<Vec<(String, PlaceholderMetadata)>>,
    }

    impl CloudFilter for FakeCloudFilter {
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
        fn create_directory(&self, path: &str) -> Result<(), String> {
            self.directories.lock().unwrap().push(path.to_owned());
            Ok(())
        }
        fn create_placeholder(
            &self,
            path: &str,
            metadata: PlaceholderMetadata,
        ) -> Result<(), String> {
            self.placeholders
                .lock()
                .unwrap()
                .push((path.to_owned(), metadata));
            Ok(())
        }
        fn acknowledge_change(
            &self,
            _: crate::virtual_library::sync_root::ChangeNotification,
            _: crate::virtual_library::sync_root::ChangeRequest,
            _: i32,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn projects_sanitized_album_directory_and_asset_placeholder() {
        let adapter = Arc::new(FakeCloudFilter::default());
        let albums = AlbumProjection::new(adapter.clone());
        let captured_at = Utc.with_ymd_and_hms(2024, 7, 3, 14, 15, 16).unwrap();

        albums
            .project(
                "C:\\Library",
                &RemoteAlbum {
                    id: "album-id".into(),
                    name: "Summer: trips?".into(),
                    assets: vec![RemoteVisualAsset {
                        id: "asset-id".into(),
                        version: "v1".into(),
                        file_name: "beach:day?.jpg".into(),
                        file_size: 42,
                        captured_at: Some(captured_at),
                        live_photo_video: None,
                    }],
                },
            )
            .unwrap();

        assert_eq!(
            *adapter.directories.lock().unwrap(),
            ["C:\\Library\\Albums", "C:\\Library\\Albums\\Summer_ trips_",]
        );
        assert_eq!(
            *adapter.placeholders.lock().unwrap(),
            [(
                "C:\\Library\\Albums\\Summer_ trips_\\beach_day_.jpg".into(),
                PlaceholderMetadata {
                    file_size: 42,
                    captured_at: Some(captured_at),
                    asset_id: "asset-id".into(),
                    version: "v1".into()
                },
            )]
        );
    }

    #[test]
    fn projects_an_album_member_separately_while_preserving_its_remote_identity() {
        let adapter = Arc::new(FakeCloudFilter::default());
        let timeline = TimelineProjection::new(adapter.clone());
        let albums = AlbumProjection::new(adapter.clone());
        let captured_at = Utc.with_ymd_and_hms(2024, 7, 3, 14, 15, 16).unwrap();
        let asset = RemoteVisualAsset {
            id: "remote-asset-id".into(),
            version: "remote-version".into(),
            file_name: "beach.jpg".into(),
            file_size: 42,
            captured_at: Some(captured_at),
            live_photo_video: None,
        };

        timeline.project("C:\\Library", &asset).unwrap();
        albums
            .project(
                "C:\\Library",
                &RemoteAlbum {
                    id: "album-id".into(),
                    name: "Summer".into(),
                    assets: vec![asset],
                },
            )
            .unwrap();

        let placeholders = adapter.placeholders.lock().unwrap();
        assert_eq!(placeholders.len(), 2);
        assert_eq!(
            placeholders[0].0,
            "C:\\Library\\All Photos\\2024\\07 July\\beach.jpg"
        );
        assert_eq!(placeholders[1].0, "C:\\Library\\Albums\\Summer\\beach.jpg");
        assert_eq!(placeholders[0].1.asset_id, "remote-asset-id");
        assert_eq!(placeholders[1].1.asset_id, "remote-asset-id");
        assert_eq!(placeholders[0].1.version, "remote-version");
        assert_eq!(placeholders[1].1.version, "remote-version");
    }
}
