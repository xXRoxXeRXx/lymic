use super::{
    filenames::FilenameResolver,
    sync_root::{CloudFilter, PlaceholderMetadata},
};
use chrono::{DateTime, Datelike, Utc};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteVisualAsset {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) file_name: String,
    pub(crate) file_size: u64,
    pub(crate) captured_at: DateTime<Utc>,
}

pub(crate) struct TimelineProjection {
    adapter: Arc<dyn CloudFilter>,
    filenames_by_directory: std::sync::Mutex<HashMap<String, FilenameResolver>>,
}

impl TimelineProjection {
    pub(crate) fn new(adapter: Arc<dyn CloudFilter>) -> Self {
        Self {
            adapter,
            filenames_by_directory: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn project(&self, root: &str, asset: &RemoteVisualAsset) -> Result<(), String> {
        let all_photos = format!("{root}\\All Photos");
        let year = format!("{}\\{}", all_photos, asset.captured_at.year());
        let month = format!(
            "{}\\{:02} {}",
            year,
            asset.captured_at.month(),
            month_name(asset.captured_at.month())
        );
        for directory in [&all_photos, &year, &month] {
            self.adapter.create_directory(directory)?;
        }
        let file_name = self
            .filenames_by_directory
            .lock()
            .map_err(|error| error.to_string())?
            .entry(month.clone())
            .or_insert_with(FilenameResolver::new)
            .resolve(&asset.file_name, &asset.id);
        self.adapter.create_placeholder(
            &format!("{month}\\{file_name}"),
            PlaceholderMetadata {
                file_size: asset.file_size,
                captured_at: asset.captured_at,
                asset_id: asset.id.clone(),
                version: asset.version.clone(),
            },
        )
    }
}

fn month_name(month: u32) -> &'static str {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .get((month - 1) as usize)
    .copied()
    .expect("chrono month is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_library::sync_root::CloudFilter;
    use chrono::{TimeZone, Utc};
    use std::sync::{Arc, Mutex};

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
    fn projects_an_online_only_placeholder_into_its_capture_month_with_remote_metadata() {
        let adapter = Arc::new(FakeCloudFilter::default());
        let timeline = TimelineProjection::new(adapter.clone());
        let asset = RemoteVisualAsset {
            id: "2f9c0a3d-1234-4567-9876-123456789abc".into(),
            version: "2026-03-04T10:11:12.000Z".into(),
            file_name: "summer:trip?.jpg".into(),
            file_size: 4_294_967_296,
            captured_at: Utc.with_ymd_and_hms(2024, 7, 3, 14, 15, 16).unwrap(),
        };

        timeline.project("C:\\Library", &asset).unwrap();

        assert_eq!(
            *adapter.directories.lock().unwrap(),
            [
                "C:\\Library\\All Photos",
                "C:\\Library\\All Photos\\2024",
                "C:\\Library\\All Photos\\2024\\07 July"
            ]
        );
        assert_eq!(
            *adapter.placeholders.lock().unwrap(),
            [(
                "C:\\Library\\All Photos\\2024\\07 July\\summer_trip_.jpg".into(),
                PlaceholderMetadata {
                    file_size: 4_294_967_296,
                    captured_at: Utc.with_ymd_and_hms(2024, 7, 3, 14, 15, 16).unwrap(),
                    asset_id: "2f9c0a3d-1234-4567-9876-123456789abc".into(),
                    version: "2026-03-04T10:11:12.000Z".into(),
                },
            )]
        );
    }

    #[test]
    fn resolves_names_independently_in_each_capture_month() {
        let adapter = Arc::new(FakeCloudFilter::default());
        let timeline = TimelineProjection::new(adapter.clone());
        let january = RemoteVisualAsset {
            id: "january-id".into(),
            version: "v1".into(),
            file_name: "same.jpg".into(),
            file_size: 1,
            captured_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        };
        let february = RemoteVisualAsset {
            id: "february-id".into(),
            version: "v1".into(),
            file_name: "same.jpg".into(),
            file_size: 1,
            captured_at: Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap(),
        };

        timeline.project("C:\\Library", &january).unwrap();
        timeline.project("C:\\Library", &february).unwrap();

        let paths: Vec<String> = adapter
            .placeholders
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        assert_eq!(
            paths,
            [
                "C:\\Library\\All Photos\\2024\\01 January\\same.jpg",
                "C:\\Library\\All Photos\\2024\\02 February\\same.jpg",
            ]
        );
    }
}
