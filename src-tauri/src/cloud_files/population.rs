//! Population: the placeholders of one directory, when Explorer first lists it (ADR-0003).
//!
//! Only the requested directory is listed through the projection (`Library::list`); its children
//! are never listed ahead of time, so opening Explorer on the root costs one cheap listing and
//! each album or month is fetched when somebody looks into it.

use super::failure::Failure;
use super::library::{Library, SizeLookup};
use super::placeholder::{self, PlaceholderPlan};
use crate::virtual_library::EntryKind;
use futures::{stream, StreamExt};

/// How many missing sizes are looked up at once.
const SIZE_LOOKUP_CONCURRENCY: usize = 8;

/// The placeholders of the directory at `path` (`""` is the root).
///
/// Immich occasionally lists an asset without its size (for instance before its metadata has been
/// extracted). Those are asked for individually; whatever still has no size is left out of the plan,
/// so a later listing, which Reconciliation will trigger, can show it correctly.
pub(super) async fn populate<L, S>(
    library: &L,
    sizes: &S,
    path: &str,
) -> Result<PlaceholderPlan, Failure>
where
    L: Library,
    S: SizeLookup,
{
    let mut entries = library.list(path).await.map_err(Failure::from_library)?;
    let unsized_files = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == EntryKind::File && entry.size.is_none())
        .filter_map(|(index, entry)| Some((index, entry.remote_asset.clone()?)))
        .collect::<Vec<_>>();
    let found = stream::iter(unsized_files)
        .map(|(index, asset)| async move { (index, sizes.size_of(&asset).await) })
        .buffer_unordered(SIZE_LOOKUP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (index, size) in found {
        entries[index].size = size;
    }
    Ok(placeholder::plan(path, &entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_files::failure::FailureKind;
    use crate::immich::RemoteAssetId;
    use crate::virtual_library::{Entry, VirtualLibraryError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn file(name: &str, id: &str, size: Option<u64>) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::File,
            remote_asset: Some(RemoteAssetId::new(id)),
            size,
            created: None,
            modified: None,
        }
    }

    fn directory(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::Directory,
            remote_asset: None,
            size: None,
            created: None,
            modified: None,
        }
    }

    /// A projection over a few directories that records which were listed.
    struct FakeLibrary {
        directories: HashMap<String, Result<Vec<Entry>, VirtualLibraryError>>,
        listed: Mutex<Vec<String>>,
    }

    impl FakeLibrary {
        fn new(directories: Vec<(&str, Result<Vec<Entry>, VirtualLibraryError>)>) -> Self {
            Self {
                directories: directories
                    .into_iter()
                    .map(|(path, entries)| (path.to_string(), entries))
                    .collect(),
                listed: Mutex::new(Vec::new()),
            }
        }
    }

    impl Library for FakeLibrary {
        async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.listed.lock().unwrap().push(path.to_string());
            self.directories
                .get(path)
                .cloned()
                .unwrap_or(Err(VirtualLibraryError::NotFound))
        }

        async fn resolve(&self, _path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            unreachable!("population never resolves")
        }
    }

    struct FakeSizes(HashMap<String, u64>);

    impl SizeLookup for FakeSizes {
        async fn size_of(&self, asset: &RemoteAssetId) -> Option<u64> {
            self.0.get(asset.as_str()).copied()
        }
    }

    fn no_sizes() -> FakeSizes {
        FakeSizes(HashMap::new())
    }

    #[tokio::test]
    async fn lists_only_the_requested_directory_and_plans_its_placeholders() {
        let library = FakeLibrary::new(vec![
            ("", Ok(vec![directory("All Photos"), directory("Albums")])),
            ("All Photos", Ok(vec![directory("2024")])),
        ]);

        let plan = populate(&library, &no_sizes(), "").await.unwrap();

        assert_eq!(
            plan.specs
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["All Photos", "Albums"]
        );
        assert_eq!(
            *library.listed.lock().unwrap(),
            vec![String::new()],
            "no child directory is listed ahead of time"
        );
    }

    #[tokio::test]
    async fn the_identity_of_every_placeholder_is_its_path_below_the_listed_directory() {
        let library = FakeLibrary::new(vec![(
            "Albums/Trip",
            Ok(vec![file("a.jpg", "id-a", Some(10))]),
        )]);

        let plan = populate(&library, &no_sizes(), "Albums/Trip")
            .await
            .unwrap();

        assert_eq!(plan.specs[0].identity, b"Albums/Trip/a.jpg".to_vec());
        assert_eq!(plan.specs[0].size, 10);
    }

    #[tokio::test]
    async fn a_file_without_a_listed_size_is_sized_by_asking_immich_for_it() {
        let library = FakeLibrary::new(vec![(
            "Albums/Trip",
            Ok(vec![
                file("a.jpg", "id-a", None),
                file("b.jpg", "id-b", Some(7)),
                file("c.jpg", "id-c", None),
            ]),
        )]);
        let sizes = FakeSizes(HashMap::from([("id-a".to_string(), 42)]));

        let plan = populate(&library, &sizes, "Albums/Trip").await.unwrap();

        assert_eq!(
            plan.specs
                .iter()
                .map(|s| (s.name.as_str(), s.size))
                .collect::<Vec<_>>(),
            vec![("a.jpg", 42), ("b.jpg", 7)]
        );
        assert_eq!(plan.skipped, vec!["c.jpg".to_string()]);
    }

    #[tokio::test]
    async fn a_listing_that_fails_fails_the_population_with_its_kind() {
        let library = FakeLibrary::new(vec![(
            "Albums",
            Err(VirtualLibraryError::Remote(
                "error sending request".to_string(),
            )),
        )]);

        let failure = populate(&library, &no_sizes(), "Albums").await.unwrap_err();

        assert_eq!(failure.kind, FailureKind::Offline);
    }

    #[tokio::test]
    async fn an_unknown_directory_fails_as_not_found() {
        let failure = populate(&FakeLibrary::new(vec![]), &no_sizes(), "Nope")
            .await
            .unwrap_err();

        assert_eq!(failure.kind, FailureKind::NotFound);
    }
}
