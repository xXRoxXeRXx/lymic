//! Filling the thumbnail cache the shell extension serves from (ADR-0003, #282).
//!
//! Explorer shows a placeholder's thumbnail through the COM provider in `lymic-shell-ext`, which
//! only reads a cache and never fetches, so that browsing cannot cause Hydration. This module is
//! the writing half: whenever the projection lists a directory, the thumbnails of its files are put
//! in the cache under the placeholder's identity (`identity`), which is what the provider looks up.
//!
//! [`WarmingLibrary`] wraps the real [`Library`] so that both places that list directories, the
//! first visit (`population`) and Reconciliation (`reconcile`), fill the cache without knowing
//! about it. The first few thumbnails are fetched *before* the listing returns, so they are there
//! when Explorer creates the placeholders and asks for them (a miss would leave a generic icon
//! until the folder is refreshed); the rest are fetched in the background. Fetching is always
//! best effort: a thumbnail that cannot be fetched never fails a listing.
//!
//! Disabling the Virtual Library or logging out clears the cache ([`clear_system_cache`]).

use super::identity;
use super::library::{Library, StoredCredentialsImmich};
use crate::immich::RemoteAssetId;
use crate::virtual_library::{Entry, EntryKind, VirtualLibraryError};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use lymic_shell_ext::cache::ThumbnailCache;
use std::future::Future;
use std::time::Duration;

/// How many thumbnails of a listing are fetched before the listing returns.
const PREWARM_COUNT: usize = 48;
/// How long that wait may take at most; what is not there by then arrives later.
const PREWARM_DEADLINE: Duration = Duration::from_secs(3);
/// Thumbnails fetched at once.
const CONCURRENCY: usize = 6;
/// What the cache may grow to before the oldest thumbnails are removed.
const CACHE_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// The thumbnail of a Remote Asset, as an image the shell can decode.
pub(super) trait ThumbnailSource: Sync {
    /// `None` when it cannot be fetched (offline, no permission, removed). Never an error: the
    /// caller has nothing to do about it.
    fn thumbnail(&self, asset: &RemoteAssetId) -> impl Future<Output = Option<Vec<u8>>> + Send;
}

impl ThumbnailSource for StoredCredentialsImmich {
    async fn thumbnail(&self, asset: &RemoteAssetId) -> Option<Vec<u8>> {
        let session = Self::session().ok()?;
        let client = session.client();
        client
            .get_thumbnail_in_format(asset.as_str(), "JPEG")
            .await
            .ok()
    }
}

/// One thumbnail to cache: the placeholder's identity and the asset it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Job {
    identity: Vec<u8>,
    asset: RemoteAssetId,
}

/// The files of the directory `parent` that have a Remote Asset and a storable identity.
pub(super) fn jobs(parent: &str, entries: &[Entry]) -> Vec<Job> {
    entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .filter_map(|entry| {
            Some(Job {
                identity: identity::encode(&identity::child_path(parent, &entry.name))?,
                asset: entry.remote_asset.clone()?,
            })
        })
        .collect()
}

/// What caching a batch did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Warmed {
    pub stored: usize,
    pub already_cached: usize,
    pub failed: usize,
}

impl Warmed {
    fn add(&mut self, other: Self) {
        self.stored += other.stored;
        self.already_cached += other.already_cached;
        self.failed += other.failed;
    }
}

async fn warm_one<S: ThumbnailSource>(cache: &ThumbnailCache, source: &S, job: &Job) -> Warmed {
    let mut outcome = Warmed::default();
    if cache.contains(&job.identity) {
        outcome.already_cached = 1;
        return outcome;
    }
    let Some(image) = source.thumbnail(&job.asset).await else {
        outcome.failed = 1;
        return outcome;
    };
    let (cache, identity) = (cache.clone(), job.identity.clone());
    let stored = tokio::task::spawn_blocking(move || cache.store(&identity, &image)).await;
    match stored {
        Ok(Ok(())) => outcome.stored = 1,
        _ => outcome.failed = 1,
    }
    outcome
}

/// Caches the thumbnails of `jobs` that are not cached yet, a few at a time.
pub(super) async fn warm<S: ThumbnailSource>(
    cache: &ThumbnailCache,
    source: &S,
    jobs: &[Job],
) -> Warmed {
    // A plain loop, not `buffer_unordered` over a closure: a closure over each borrowed job makes
    // the future fail the higher-ranked `Send` check of `tokio::spawn`.
    let mut total = Warmed::default();
    let mut running = FuturesUnordered::new();
    let mut waiting = jobs.iter();
    loop {
        while running.len() < CONCURRENCY {
            let Some(job) = waiting.next() else { break };
            running.push(warm_one(cache, source, job));
        }
        match running.next().await {
            Some(outcome) => total.add(outcome),
            None => return total,
        }
    }
}

/// A [`Library`] that caches the thumbnails of every directory it lists.
pub(super) struct WarmingLibrary<L, S> {
    inner: L,
    source: S,
    /// `None` (no Package Identity) makes this a plain pass-through.
    cache: Option<ThumbnailCache>,
    pub(super) prewarm_count: usize,
    pub(super) prewarm_deadline: Duration,
}

impl<L, S> WarmingLibrary<L, S> {
    pub(super) fn new(inner: L, source: S, cache: Option<ThumbnailCache>) -> Self {
        Self {
            inner,
            source,
            cache,
            prewarm_count: PREWARM_COUNT,
            prewarm_deadline: PREWARM_DEADLINE,
        }
    }
}

impl<L> WarmingLibrary<L, StoredCredentialsImmich> {
    /// Over the real Immich and the running package's cache.
    pub(super) fn system(inner: L) -> Self {
        Self::new(inner, StoredCredentialsImmich, system_cache())
    }
}

impl<L, S> Library for WarmingLibrary<L, S>
where
    L: Library,
    S: ThumbnailSource + Clone + Send + 'static,
{
    async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
        let entries = self.inner.list(path).await?;
        let Some(cache) = &self.cache else {
            return Ok(entries);
        };
        let mut jobs = jobs(path, &entries);
        let later = jobs.split_off(self.prewarm_count.min(jobs.len()));
        // Not finishing in time is fine: those thumbnails are simply missing for now.
        let _ = tokio::time::timeout(self.prewarm_deadline, warm(cache, &self.source, &jobs)).await;
        let (cache, source) = (cache.clone(), self.source.clone());
        tokio::spawn(async move {
            warm(&cache, &source, &later).await;
            let _ = tokio::task::spawn_blocking(move || cache.trim(CACHE_LIMIT_BYTES)).await;
        });
        Ok(entries)
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        self.inner.resolve(path).await
    }
}

/// The cache of the running package, `None` without Package Identity (and off Windows).
fn system_cache() -> Option<ThumbnailCache> {
    #[cfg(windows)]
    {
        lymic_shell_ext::shell::package_cache()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Deletes the cached thumbnails: nothing of the account stays behind when the Virtual Library is
/// disabled or the user logs out. Best effort; a failure leaves files that the next run overwrites.
pub(super) fn clear_system_cache() {
    if let Some(cache) = system_cache() {
        if let Err(error) = cache.clear() {
            eprintln!("virtual library: could not clear the thumbnail cache: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F'];

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            Self(std::env::temp_dir().join(format!(
                "lymic-shell-thumbnails-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }

        fn cache(&self) -> ThumbnailCache {
            ThumbnailCache::new(self.0.join("cache"))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Serves a thumbnail for the assets it was given; `hang` never answers for those.
    #[derive(Clone, Default)]
    struct FakeSource {
        images: Arc<HashMap<String, Vec<u8>>>,
        hang: Arc<HashSet<String>>,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl FakeSource {
        fn with(images: &[(&str, &[u8])]) -> Self {
            Self {
                images: Arc::new(
                    images
                        .iter()
                        .map(|(id, bytes)| (id.to_string(), bytes.to_vec()))
                        .collect(),
                ),
                ..Self::default()
            }
        }

        fn hanging(mut self, id: &str) -> Self {
            self.hang = Arc::new(HashSet::from([id.to_string()]));
            self
        }

        fn asked(&self) -> Vec<String> {
            let mut asked = self.asked.lock().unwrap().clone();
            asked.sort();
            asked
        }
    }

    impl ThumbnailSource for FakeSource {
        async fn thumbnail(&self, asset: &RemoteAssetId) -> Option<Vec<u8>> {
            self.asked.lock().unwrap().push(asset.as_str().to_string());
            if self.hang.contains(asset.as_str()) {
                std::future::pending::<()>().await;
            }
            self.images.get(asset.as_str()).cloned()
        }
    }

    struct FakeLibrary(Result<Vec<Entry>, VirtualLibraryError>);

    impl Library for FakeLibrary {
        async fn list(&self, _path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.0.clone()
        }

        async fn resolve(&self, _path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            Ok(RemoteAssetId::new("resolved"))
        }
    }

    fn file(name: &str, asset: Option<&str>) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::File,
            remote_asset: asset.map(RemoteAssetId::new),
            size: Some(10),
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

    fn library(
        entries: Vec<Entry>,
        source: &FakeSource,
        dir: &TempDir,
    ) -> WarmingLibrary<FakeLibrary, FakeSource> {
        WarmingLibrary::new(FakeLibrary(Ok(entries)), source.clone(), Some(dir.cache()))
    }

    #[test]
    fn only_files_with_an_asset_become_jobs_identified_by_their_path() {
        let entries = vec![
            directory("03 March"),
            file("IMG_0001.jpg", Some("asset-1")),
            file("orphan.jpg", None),
        ];

        let jobs = jobs("All Photos/2024", &entries);

        assert_eq!(
            jobs,
            vec![Job {
                identity: b"All Photos/2024/IMG_0001.jpg".to_vec(),
                asset: RemoteAssetId::new("asset-1"),
            }]
        );
    }

    #[tokio::test]
    async fn listing_returns_the_entries_unchanged_and_caches_their_thumbnails() {
        let dir = TempDir::new();
        let source = FakeSource::with(&[("asset-1", JPEG), ("asset-2", JPEG)]);
        let entries = vec![
            directory("Sub"),
            file("a.jpg", Some("asset-1")),
            file("b.jpg", Some("asset-2")),
        ];
        let library = library(entries.clone(), &source, &dir);

        let listed = library.list("Albums/Trip").await.unwrap();

        assert_eq!(listed, entries);
        let cache = dir.cache();
        assert_eq!(cache.load(b"Albums/Trip/a.jpg").unwrap(), JPEG);
        assert_eq!(cache.load(b"Albums/Trip/b.jpg").unwrap(), JPEG);
        assert!(!cache.contains(b"Albums/Trip/Sub"));
    }

    #[tokio::test]
    async fn a_thumbnail_that_is_already_cached_is_not_fetched_again() {
        let dir = TempDir::new();
        dir.cache().store(b"a.jpg", JPEG).unwrap();
        let source = FakeSource::with(&[("asset-1", JPEG), ("asset-2", JPEG)]);
        let library = library(
            vec![
                file("a.jpg", Some("asset-1")),
                file("b.jpg", Some("asset-2")),
            ],
            &source,
            &dir,
        );

        library.list("").await.unwrap();

        assert_eq!(source.asked(), vec!["asset-2".to_string()]);
    }

    #[tokio::test]
    async fn a_thumbnail_that_cannot_be_fetched_or_is_no_image_never_fails_the_listing() {
        let dir = TempDir::new();
        let source = FakeSource::with(&[
            ("good", JPEG),
            ("error-page", b"{\"message\":\"not found\"}"),
        ]);
        let library = library(
            vec![
                file("missing.jpg", Some("gone")),
                file("error.jpg", Some("error-page")),
                file("good.jpg", Some("good")),
            ],
            &source,
            &dir,
        );

        let listed = library.list("").await.unwrap();

        assert_eq!(listed.len(), 3);
        let cache = dir.cache();
        assert!(cache.contains(b"good.jpg"));
        assert!(!cache.contains(b"missing.jpg"));
        assert!(!cache.contains(b"error.jpg"));
    }

    #[tokio::test]
    async fn warming_counts_what_it_stored_skipped_and_failed() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.store(b"cached.jpg", JPEG).unwrap();
        let source = FakeSource::with(&[("a", JPEG)]);
        let jobs = jobs(
            "",
            &[
                file("cached.jpg", Some("c")),
                file("new.jpg", Some("a")),
                file("fail.jpg", Some("x")),
            ],
        );

        let warmed = warm(&cache, &source, &jobs).await;

        assert_eq!(
            warmed,
            Warmed {
                stored: 1,
                already_cached: 1,
                failed: 1
            }
        );
    }

    #[tokio::test]
    async fn a_slow_thumbnail_holds_the_listing_back_only_until_the_deadline() {
        let dir = TempDir::new();
        let source = FakeSource::with(&[("fast", JPEG)]).hanging("slow");
        let mut library = library(
            vec![
                file("fast.jpg", Some("fast")),
                file("slow.jpg", Some("slow")),
            ],
            &source,
            &dir,
        );
        library.prewarm_deadline = Duration::from_millis(100);

        let listed = tokio::time::timeout(Duration::from_secs(5), library.list(""))
            .await
            .expect("the listing must not wait for a thumbnail that never arrives")
            .unwrap();

        assert_eq!(listed.len(), 2);
        assert!(dir.cache().contains(b"fast.jpg"));
        assert!(!dir.cache().contains(b"slow.jpg"));
    }

    #[tokio::test]
    async fn thumbnails_beyond_the_prewarm_count_arrive_in_the_background() {
        let dir = TempDir::new();
        let source = FakeSource::with(&[("1", JPEG), ("2", JPEG), ("3", JPEG)]);
        let mut library = library(
            vec![
                file("1.jpg", Some("1")),
                file("2.jpg", Some("2")),
                file("3.jpg", Some("3")),
            ],
            &source,
            &dir,
        );
        library.prewarm_count = 1;

        library.list("").await.unwrap();

        let cache = dir.cache();
        assert!(cache.contains(b"1.jpg"), "the first one is there on return");
        for _ in 0..100 {
            if cache.contains(b"2.jpg") && cache.contains(b"3.jpg") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the remaining thumbnails never arrived");
    }

    #[tokio::test]
    async fn without_a_cache_listing_just_passes_through() {
        let source = FakeSource::with(&[("asset-1", JPEG)]);
        let library = WarmingLibrary::new(
            FakeLibrary(Ok(vec![file("a.jpg", Some("asset-1"))])),
            source.clone(),
            None,
        );

        assert_eq!(library.list("").await.unwrap().len(), 1);
        assert!(source.asked().is_empty());
    }

    #[tokio::test]
    async fn a_failed_listing_and_a_resolve_pass_through() {
        let dir = TempDir::new();
        let library = WarmingLibrary::new(
            FakeLibrary(Err(VirtualLibraryError::NotFound)),
            FakeSource::default(),
            Some(dir.cache()),
        );

        assert!(matches!(
            library.list("x").await,
            Err(VirtualLibraryError::NotFound)
        ));
        assert_eq!(
            library.resolve("x").await.unwrap(),
            RemoteAssetId::new("resolved")
        );
    }
}
