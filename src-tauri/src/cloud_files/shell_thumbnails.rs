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
//! Prewarming is not enough on its own (it is capped at 48 thumbnails per listing, runs once and an
//! Explorer window can ask for any file at any time), so the provider also leaves a request for every
//! thumbnail it was asked for and did not find ([`ThumbnailCache::record_miss`]). [`serve_misses`]
//! turns those requests into cached thumbnails and refreshes the item in Explorer
//! ([`ItemRefresh`]); `run_miss_server` polls for them while the sync root is connected. A fetch
//! that fails is retried ([`Retry`]), and a request that still cannot be served is asked again for a
//! few rounds before it is given up.
//!
//! Disabling the Virtual Library or logging out clears the cache ([`clear_system_cache`]).

use super::diagnostic_log::{NoNotes, Notes, SharedNotes, SystemNotes};
use super::identity;
use super::library::{Library, StoredCredentialsImmich};
use super::listing_log::ListingLog;
use crate::immich::RemoteAssetId;
use crate::virtual_library::{Entry, EntryKind, VirtualLibraryError};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use lymic_shell_ext::cache::{Miss, ThumbnailCache};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// How many thumbnails of a listing are fetched before the listing returns.
const PREWARM_COUNT: usize = 48;
/// How long that wait may take at most; what is not there by then arrives later.
const PREWARM_DEADLINE: Duration = Duration::from_secs(3);
/// Thumbnails fetched at once.
const CONCURRENCY: usize = 6;
/// What the cache may grow to before the least recently used thumbnails are removed.
const CACHE_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
/// How often the requests Explorer leaves for missing thumbnails are looked at.
const MISS_POLL: Duration = Duration::from_secs(2);
/// How many times a request that cannot be served is asked again before it is given up.
const MISS_ROUNDS: u32 = 3;

/// How a thumbnail fetch that returned nothing is tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Retry {
    /// Tries in all, the first included.
    pub attempts: u32,
    /// Pause after a failed try, multiplied by the number of tries so far.
    pub delay: Duration,
}

/// A transient failure (a dropped connection, a server still generating the thumbnail) usually
/// passes within seconds.
pub(super) const RETRY: Retry = Retry {
    attempts: 3,
    delay: Duration::from_secs(2),
};

/// The thumbnail of a Remote Asset, as an image the shell can decode.
pub(super) trait ThumbnailSource: Sync {
    /// `None` when it cannot be fetched (offline, no permission, removed). Never an error: the
    /// caller has nothing to do about it.
    fn thumbnail(&self, asset: &RemoteAssetId) -> impl Future<Output = Option<Vec<u8>>> + Send;
}

impl ThumbnailSource for StoredCredentialsImmich {
    async fn thumbnail(&self, asset: &RemoteAssetId) -> Option<Vec<u8>> {
        let session = match Self::session() {
            Ok(session) => session,
            Err(failure) => {
                SystemNotes.note(&format!(
                    "thumbnail of asset {}: no session: {}",
                    asset.as_str(),
                    failure.message
                ));
                return None;
            }
        };
        let client = session.client();
        match client.get_thumbnail_in_format(asset.as_str(), "JPEG").await {
            Ok(image) => Some(image),
            Err(error) => {
                SystemNotes.note(&format!(
                    "thumbnail of asset {}: Immich did not deliver it: {}",
                    asset.as_str(),
                    session.credentials().redact(&error)
                ));
                None
            }
        }
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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Warmed {
    pub stored: usize,
    pub already_cached: usize,
    pub failed: usize,
    /// Why the first thumbnail that failed did, for the log.
    pub first_failure: Option<String>,
}

impl Warmed {
    fn add(&mut self, other: Self) {
        self.stored += other.stored;
        self.already_cached += other.already_cached;
        self.failed += other.failed;
        if self.first_failure.is_none() {
            self.first_failure = other.first_failure;
        }
    }

    /// `3 cached, 1 already cached, 2 failed (first failure: ...)`.
    pub(super) fn describe(&self) -> String {
        let mut text = format!(
            "{} cached, {} already cached, {} failed",
            self.stored, self.already_cached, self.failed
        );
        if let Some(reason) = &self.first_failure {
            text.push_str(&format!(" (first failure: {reason})"));
        }
        text
    }
}

async fn warm_one<S: ThumbnailSource>(
    cache: &ThumbnailCache,
    source: &S,
    job: &Job,
    retry: Retry,
) -> Warmed {
    let mut outcome = Warmed::default();
    if cache.contains(&job.identity) {
        outcome.already_cached = 1;
        return outcome;
    }
    let mut image = None;
    for attempt in 1..=retry.attempts.max(1) {
        image = source.thumbnail(&job.asset).await;
        if image.is_some() || attempt >= retry.attempts {
            break;
        }
        tokio::time::sleep(retry.delay * attempt).await;
    }
    let name = String::from_utf8_lossy(&job.identity).into_owned();
    let Some(image) = image else {
        outcome.failed = 1;
        outcome.first_failure = Some(format!("no thumbnail from Immich for '{name}'"));
        return outcome;
    };
    let (cache, identity) = (cache.clone(), job.identity.clone());
    let stored = tokio::task::spawn_blocking(move || cache.store(&identity, &image)).await;
    match stored {
        Ok(Ok(())) => outcome.stored = 1,
        Ok(Err(error)) => {
            outcome.failed = 1;
            outcome.first_failure = Some(format!("'{name}' was not stored: {error}"));
        }
        Err(error) => {
            outcome.failed = 1;
            outcome.first_failure = Some(format!("'{name}' was not stored: {error}"));
        }
    }
    outcome
}

/// Caches the thumbnails of `jobs` that are not cached yet, a few at a time, trying each as
/// `retry` says.
pub(super) async fn warm<S: ThumbnailSource>(
    cache: &ThumbnailCache,
    source: &S,
    jobs: &[Job],
    retry: Retry,
) -> Warmed {
    let mut total = Warmed::default();
    for outcome in warm_each(cache, source, jobs, retry).await {
        total.add(outcome);
    }
    total
}

/// [`warm`], with the outcome of every job, in the order of `jobs`.
pub(super) async fn warm_each<S: ThumbnailSource>(
    cache: &ThumbnailCache,
    source: &S,
    jobs: &[Job],
    retry: Retry,
) -> Vec<Warmed> {
    // A plain loop, not `buffer_unordered` over a closure: a closure over each borrowed job makes
    // the future fail the higher-ranked `Send` check of `tokio::spawn`.
    let mut outcomes: Vec<Warmed> = jobs.iter().map(|_| Warmed::default()).collect();
    let mut running = FuturesUnordered::new();
    let mut waiting = jobs.iter().enumerate();
    loop {
        while running.len() < CONCURRENCY {
            let Some((index, job)) = waiting.next() else {
                break;
            };
            running.push(async move { (index, warm_one(cache, source, job, retry).await) });
        }
        match running.next().await {
            Some((index, outcome)) => outcomes[index] = outcome,
            None => return outcomes,
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
    pub(super) retry: Retry,
    pub(super) notes: SharedNotes,
}

impl<L, S> WarmingLibrary<L, S> {
    pub(super) fn new(inner: L, source: S, cache: Option<ThumbnailCache>) -> Self {
        Self {
            inner,
            source,
            cache,
            prewarm_count: PREWARM_COUNT,
            prewarm_deadline: PREWARM_DEADLINE,
            retry: RETRY,
            notes: Arc::new(NoNotes),
        }
    }
}

impl<L> WarmingLibrary<L, StoredCredentialsImmich> {
    /// Over the real Immich and the running package's cache.
    pub(super) fn system(inner: L) -> Self {
        let mut library = Self::new(inner, StoredCredentialsImmich, system_cache());
        library.notes = Arc::new(SystemNotes);
        library
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
            self.notes.note(&format!(
                "no thumbnail cache (no package identity): '{path}' is listed without thumbnails"
            ));
            return Ok(entries);
        };
        let mut jobs = jobs(path, &entries);
        let later = jobs.split_off(self.prewarm_count.min(jobs.len()));
        let (first, rest) = (jobs.len(), later.len());
        // The first thumbnails are fetched on a task of their own: when the deadline passes the
        // listing returns and the task carries on, instead of being dropped half way, so what is
        // not there by then arrives later.
        let mut prewarm = {
            let (cache, source, retry) = (cache.clone(), self.source.clone(), self.retry);
            tokio::spawn(async move { warm(&cache, &source, &jobs, retry).await })
        };
        let finished = tokio::time::timeout(self.prewarm_deadline, &mut prewarm).await;
        let (cache, source, retry, notes) = (
            cache.clone(),
            self.source.clone(),
            self.retry,
            self.notes.clone(),
        );
        let path = path.to_string();
        match &finished {
            Ok(Ok(warmed)) => notes.note(&format!(
                "thumbnails of '{path}': the first {first} before the listing returned: {}",
                warmed.describe()
            )),
            Ok(Err(error)) => notes.note(&format!(
                "thumbnails of '{path}': the first {first} stopped with an error: {error}"
            )),
            Err(_) => notes.note(&format!(
                "thumbnails of '{path}': the first {first} were not all there after {} ms; they continue in the background",
                self.prewarm_deadline.as_millis()
            )),
        }
        let late = finished.is_err();
        tokio::spawn(async move {
            if late {
                match prewarm.await {
                    Ok(warmed) => notes.note(&format!(
                        "thumbnails of '{path}': the first {first}, finished late: {}",
                        warmed.describe()
                    )),
                    Err(error) => notes.note(&format!(
                        "thumbnails of '{path}': the first {first} stopped with an error: {error}"
                    )),
                }
            }
            if rest > 0 {
                let warmed = warm(&cache, &source, &later, retry).await;
                notes.note(&format!(
                    "thumbnails of '{path}': the other {rest} in the background: {}",
                    warmed.describe()
                ));
            }
            let _ = tokio::task::spawn_blocking(move || cache.trim(CACHE_LIMIT_BYTES)).await;
        });
        Ok(entries)
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        self.inner.resolve(path).await
    }

    async fn explain_missing(&self, path: &str) -> Option<String> {
        self.inner.explain_missing(path).await
    }
}

/// Tells Explorer that an item changed, so that it asks for its thumbnail again.
pub(super) trait ItemRefresh: Sync {
    /// `path` is the item's full file system path.
    fn refresh(&self, path: &str);
}

/// How often each request that could not be served has been tried, so that a thumbnail that cannot
/// be had is not asked for for ever.
#[derive(Debug, Default)]
pub(super) struct MissQueue {
    rounds: HashMap<Vec<u8>, u32>,
}

/// What one pass over the requests did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Served {
    /// Thumbnails now cached (and their items refreshed).
    pub stored: usize,
    /// Requests that failed and were left to be tried again.
    pub requeued: usize,
    /// Requests given up: no path, or too many failed rounds.
    pub dropped: usize,
}

/// Serves the thumbnails Explorer asked for and the cache did not have: the provider answered those
/// with a generic image and left a request. The requests are grouped by directory and each
/// directory is listed once (`identity` to name to Remote Asset, as `Library::resolve` would, without
/// listing a month again for every file of it), the thumbnails are fetched a few at a time with
/// `retry` and stored, and each item is refreshed so Explorer replaces the generic image. A request
/// that cannot be served is put back for another round, up to [`MISS_ROUNDS`] rounds, because
/// Explorer keeps the generic image it was given and will not ask again by itself. Every directory
/// served, and every request that cannot be, is written to `notes`.
pub(super) async fn serve_misses<L, S, R>(
    cache: &ThumbnailCache,
    library: &L,
    source: &S,
    refresh: &R,
    queue: &mut MissQueue,
    retry: Retry,
    notes: &dyn Notes,
) -> Served
where
    L: Library,
    S: ThumbnailSource,
    R: ItemRefresh,
{
    let mut served = Served::default();
    let mut directories: BTreeMap<String, Vec<(Miss, String)>> = BTreeMap::new();
    for miss in cache.take_misses() {
        let Some(path) = identity::decode(&miss.identity) else {
            served.dropped += 1;
            notes.note(&format!(
                "gave up on a thumbnail request for '{}': its identity is not a path",
                miss.path
            ));
            continue;
        };
        let (directory, _) = path.rsplit_once('/').unwrap_or(("", &path));
        directories
            .entry(directory.to_string())
            .or_default()
            .push((miss, path));
    }
    for (directory, requests) in directories {
        let asked = requests.len();
        let before = served;
        let outcomes = resolve_and_warm(cache, library, source, &directory, &requests, retry).await;
        let mut first_failure = None;
        for ((miss, path), outcome) in requests.into_iter().zip(outcomes) {
            let failure = match outcome {
                Ok(()) => {
                    queue.rounds.remove(&miss.identity);
                    refresh.refresh(&miss.path);
                    served.stored += 1;
                    continue;
                }
                Err(failure) => failure,
            };
            first_failure.get_or_insert_with(|| format!("'{path}': {failure}"));
            let rounds = queue.rounds.entry(miss.identity.clone()).or_insert(0);
            *rounds += 1;
            if *rounds >= MISS_ROUNDS {
                queue.rounds.remove(&miss.identity);
                served.dropped += 1;
                notes.note(&format!(
                    "gave up on '{path}' after {MISS_ROUNDS} rounds: {failure}"
                ));
            } else {
                cache.record_miss(&miss.identity, &miss.path);
                served.requeued += 1;
            }
        }
        let mut line = format!(
            "thumbnail requests for '{directory}': {asked} asked, {} cached, {} to try again, {} given up",
            served.stored - before.stored,
            served.requeued - before.requeued,
            served.dropped - before.dropped
        );
        if let Some(failure) = first_failure {
            line.push_str(&format!(" (first failure: {failure})"));
        }
        notes.note(&line);
    }
    served
}

/// Looks the requested files up in one listing of `directory` and caches their thumbnails, a few at
/// a time. Each result is `Ok` or why that request could not be served.
async fn resolve_and_warm<L, S>(
    cache: &ThumbnailCache,
    library: &L,
    source: &S,
    directory: &str,
    requests: &[(Miss, String)],
    retry: Retry,
) -> Vec<Result<(), String>>
where
    L: Library,
    S: ThumbnailSource,
{
    let entries = match library.list(directory).await {
        Ok(entries) => entries,
        Err(error) => {
            let why = library.explain_missing(directory).await;
            let reason = format!(
                "listing '{directory}' failed: {error:?}{}",
                why.map(|why| format!(" ({why})")).unwrap_or_default()
            );
            return requests.iter().map(|_| Err(reason.clone())).collect();
        }
    };
    let count = entries.len();
    let mut results: Vec<Result<(), String>> = Vec::with_capacity(requests.len());
    let mut jobs = Vec::new();
    for (index, (miss, path)) in requests.iter().enumerate() {
        let name = path
            .rsplit_once('/')
            .map_or(path.as_str(), |(_, name)| name);
        let wanted = name.to_lowercase();
        let asset = entries
            .iter()
            .find(|entry| entry.kind == EntryKind::File && entry.name.to_lowercase() == wanted)
            .and_then(|entry| entry.remote_asset.clone());
        match asset {
            Some(asset) => {
                results.push(Ok(()));
                jobs.push((
                    index,
                    Job {
                        identity: miss.identity.clone(),
                        asset,
                    },
                ));
            }
            None => results.push(Err(format!(
                "no file of that name among the {count} entries of '{directory}'"
            ))),
        }
    }
    let only_jobs: Vec<Job> = jobs.iter().map(|(_, job)| job.clone()).collect();
    for ((index, _), warmed) in jobs
        .iter()
        .zip(warm_each(cache, source, &only_jobs, retry).await)
    {
        if warmed.stored + warmed.already_cached == 0 {
            results[*index] = Err(warmed
                .first_failure
                .unwrap_or_else(|| "no thumbnail from Immich".to_string()));
        }
    }
    results
}

/// Serves the requests of the running package's cache until the task is aborted: the app calls it
/// when the sync root is connected and aborts it when it stops. `refresh` is the adapter's
/// shell notification.
pub(super) async fn run_miss_server<R: ItemRefresh>(refresh: R) {
    let Some(cache) = system_cache() else {
        SystemNotes.note(
            "the thumbnail request server does not run: no thumbnail cache (no package identity)",
        );
        return;
    };
    SystemNotes.note("the thumbnail request server runs");
    let library = ListingLog::new(StoredCredentialsImmich);
    let mut queue = MissQueue::default();
    loop {
        tokio::time::sleep(MISS_POLL).await;
        let served = serve_misses(
            &cache,
            &library,
            &StoredCredentialsImmich,
            &refresh,
            &mut queue,
            RETRY,
            &SystemNotes,
        )
        .await;
        if served.stored > 0 {
            let cache = cache.clone();
            let _ = tokio::task::spawn_blocking(move || cache.trim(CACHE_LIMIT_BYTES)).await;
        }
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
        let mut library =
            WarmingLibrary::new(FakeLibrary(Ok(entries)), source.clone(), Some(dir.cache()));
        library.retry = FAST;
        library
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

        let warmed = warm(&cache, &source, &jobs, FAST).await;

        assert_eq!(
            (warmed.stored, warmed.already_cached, warmed.failed),
            (1, 1, 1)
        );
        assert_eq!(
            warmed.first_failure.as_deref(),
            Some("no thumbnail from Immich for 'fail.jpg'")
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

    /// Answers like [`FakeSource`], but only after `delay`.
    #[derive(Clone)]
    struct DelayedSource {
        inner: FakeSource,
        delay: Duration,
    }

    impl ThumbnailSource for DelayedSource {
        async fn thumbnail(&self, asset: &RemoteAssetId) -> Option<Vec<u8>> {
            tokio::time::sleep(self.delay).await;
            self.inner.thumbnail(asset).await
        }
    }

    #[tokio::test]
    async fn a_prewarm_that_misses_the_deadline_is_finished_in_the_background_not_abandoned() {
        let dir = TempDir::new();
        let source = DelayedSource {
            inner: FakeSource::with(&[("1", JPEG), ("2", JPEG)]),
            delay: Duration::from_millis(300),
        };
        let mut library = WarmingLibrary::new(
            FakeLibrary(Ok(vec![file("1.jpg", Some("1")), file("2.jpg", Some("2"))])),
            source,
            Some(dir.cache()),
        );
        library.retry = FAST;
        library.prewarm_deadline = Duration::from_millis(50);

        let listed = library.list("").await.unwrap();

        assert_eq!(listed.len(), 2, "the listing does not wait for them");
        let cache = dir.cache();
        for _ in 0..100 {
            if cache.contains(b"1.jpg") && cache.contains(b"2.jpg") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the thumbnails the deadline cut short never arrived");
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

    // ------------------------------------------------------------------ retries and misses

    const FAST: Retry = Retry {
        attempts: 3,
        delay: Duration::from_millis(1),
    };

    /// Answers like [`FakeSource`], except that the first `times` asks for `id` get nothing.
    #[derive(Clone)]
    struct FlakySource {
        inner: FakeSource,
        failures_left: Arc<Mutex<HashMap<String, usize>>>,
    }

    impl FlakySource {
        fn new(inner: FakeSource, id: &str, times: usize) -> Self {
            Self {
                inner,
                failures_left: Arc::new(Mutex::new(HashMap::from([(id.to_string(), times)]))),
            }
        }
    }

    impl ThumbnailSource for FlakySource {
        async fn thumbnail(&self, asset: &RemoteAssetId) -> Option<Vec<u8>> {
            {
                let mut left = self.failures_left.lock().unwrap();
                if let Some(remaining) = left.get_mut(asset.as_str()) {
                    if *remaining > 0 {
                        *remaining -= 1;
                        return None;
                    }
                }
            }
            self.inner.thumbnail(asset).await
        }
    }

    #[tokio::test]
    async fn a_thumbnail_that_fails_at_first_is_tried_again_and_cached() {
        let dir = TempDir::new();
        let cache = dir.cache();
        let source = FlakySource::new(FakeSource::with(&[("a", JPEG)]), "a", 2);
        let jobs = jobs("", &[file("a.jpg", Some("a"))]);

        let warmed = warm(&cache, &source, &jobs, FAST).await;

        assert_eq!(warmed.stored, 1);
        assert_eq!(warmed.failed, 0);
        assert!(cache.contains(b"a.jpg"));
    }

    #[tokio::test]
    async fn a_thumbnail_that_keeps_failing_is_given_up_after_the_attempts() {
        let dir = TempDir::new();
        let cache = dir.cache();
        let source = FakeSource::with(&[]);
        let jobs = jobs("", &[file("a.jpg", Some("gone"))]);

        let warmed = warm(&cache, &source, &jobs, FAST).await;

        assert_eq!(warmed.failed, 1);
        assert_eq!(source.asked().len(), 3, "three attempts, no more");
    }

    #[test]
    fn a_video_is_a_thumbnail_job_like_a_photo() {
        // Immich renders a JPEG thumbnail for a video too (`format=JPEG`), so a `.3gp` is cached
        // like any photo and is not special-cased.
        let jobs = jobs(
            "All Photos/Undated",
            &[file("Garfild.3gp", Some("video-1"))],
        );

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].identity, b"All Photos/Undated/Garfild.3gp".to_vec());
    }

    /// Holds the files it was given by path; resolving one lists its directory, as the real
    /// projection does, and every listing is counted.
    struct PathLibrary {
        files: HashMap<String, String>,
        listings: AtomicUsize,
    }

    impl PathLibrary {
        fn of(paths: &[(&str, &str)]) -> Self {
            Self {
                files: paths
                    .iter()
                    .map(|(path, asset)| (path.to_string(), asset.to_string()))
                    .collect(),
                listings: AtomicUsize::new(0),
            }
        }

        fn listings(&self) -> usize {
            self.listings.load(Ordering::SeqCst)
        }

        fn entries_of(&self, directory: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.listings.fetch_add(1, Ordering::SeqCst);
            let prefix = if directory.is_empty() {
                String::new()
            } else {
                format!("{directory}/")
            };
            let entries: Vec<Entry> = self
                .files
                .iter()
                .filter_map(|(path, asset)| {
                    let name = path.strip_prefix(&prefix)?;
                    (!name.contains('/')).then(|| file(name, Some(asset)))
                })
                .collect();
            if entries.is_empty() {
                Err(VirtualLibraryError::NotFound)
            } else {
                Ok(entries)
            }
        }
    }

    impl Library for PathLibrary {
        async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            self.entries_of(path)
        }

        async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            let (directory, name) = path.rsplit_once('/').unwrap_or(("", path));
            self.entries_of(directory)?
                .into_iter()
                .find(|entry| entry.name == name)
                .and_then(|entry| entry.remote_asset)
                .ok_or(VirtualLibraryError::NotFound)
        }
    }

    #[derive(Default)]
    struct RecordedNotes(Mutex<Vec<String>>);

    impl Notes for RecordedNotes {
        fn note(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    impl RecordedNotes {
        fn text(&self) -> String {
            self.0.lock().unwrap().join("\n")
        }
    }

    #[derive(Default)]
    struct RecordingRefresh(Mutex<Vec<String>>);

    impl ItemRefresh for RecordingRefresh {
        fn refresh(&self, path: &str) {
            self.0.lock().unwrap().push(path.to_string());
        }
    }

    #[tokio::test]
    async fn a_thumbnail_explorer_asked_for_is_cached_and_the_item_refreshed() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"Albums/Trip/a.jpg", r"C:\Lymic\Albums\Trip\a.jpg");
        let library = PathLibrary::of(&[("Albums/Trip/a.jpg", "asset-1")]);
        let source = FakeSource::with(&[("asset-1", JPEG)]);
        let refresh = RecordingRefresh::default();

        let served = serve_misses(
            &cache,
            &library,
            &source,
            &refresh,
            &mut MissQueue::default(),
            FAST,
            &NoNotes,
        )
        .await;

        assert_eq!(served.stored, 1);
        assert!(cache.contains(b"Albums/Trip/a.jpg"));
        assert_eq!(
            *refresh.0.lock().unwrap(),
            vec![r"C:\Lymic\Albums\Trip\a.jpg".to_string()]
        );
        assert!(cache.take_misses().is_empty(), "the request is done");
    }

    #[tokio::test]
    async fn a_request_that_cannot_be_served_is_asked_again_later_and_dropped_after_three_rounds() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"a.jpg", r"C:\Lymic\a.jpg");
        let library = PathLibrary::of(&[("a.jpg", "gone")]);
        let source = FakeSource::with(&[]);
        let refresh = RecordingRefresh::default();
        let mut queue = MissQueue::default();

        let first = serve_misses(
            &cache, &library, &source, &refresh, &mut queue, FAST, &NoNotes,
        )
        .await;
        let second = serve_misses(
            &cache, &library, &source, &refresh, &mut queue, FAST, &NoNotes,
        )
        .await;
        let third = serve_misses(
            &cache, &library, &source, &refresh, &mut queue, FAST, &NoNotes,
        )
        .await;
        let fourth = serve_misses(
            &cache, &library, &source, &refresh, &mut queue, FAST, &NoNotes,
        )
        .await;

        assert_eq!((first.requeued, first.dropped), (1, 0));
        assert_eq!((second.requeued, second.dropped), (1, 0));
        assert_eq!((third.requeued, third.dropped), (0, 1));
        assert_eq!(fourth, Served::default());
        assert!(refresh.0.lock().unwrap().is_empty());
        assert!(cache.take_misses().is_empty(), "nothing is left to retry");
    }

    #[tokio::test]
    async fn a_request_for_a_path_that_no_longer_resolves_is_dropped_in_the_end() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"removed.jpg", r"C:\Lymic\removed.jpg");
        let library = PathLibrary::of(&[]);
        let source = FakeSource::with(&[]);
        let refresh = RecordingRefresh::default();
        let mut queue = MissQueue::default();

        for _ in 0..3 {
            serve_misses(
                &cache, &library, &source, &refresh, &mut queue, FAST, &NoNotes,
            )
            .await;
        }

        assert!(cache.take_misses().is_empty());
        assert!(
            source.asked().is_empty(),
            "nothing to fetch for an unknown asset"
        );
    }

    #[tokio::test]
    async fn a_request_whose_identity_is_not_a_path_is_dropped_at_once() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(&[0xFF, 0xFE, 0x00], r"C:\Lymic\x");

        let served = serve_misses(
            &cache,
            &PathLibrary::of(&[]),
            &FakeSource::with(&[]),
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &NoNotes,
        )
        .await;

        assert_eq!(served.dropped, 1);
        assert!(cache.take_misses().is_empty());
    }

    #[tokio::test]
    async fn the_thumbnails_asked_for_in_one_month_list_that_month_once_not_once_per_file() {
        let dir = TempDir::new();
        let cache = dir.cache();
        let mut paths = Vec::new();
        let mut images = Vec::new();
        for number in 0..12 {
            let path = format!("All Photos/2016/04 April/2016-04-29_1824{number:02}_000.mp4");
            cache.record_miss(path.as_bytes(), &format!(r"C:\Lymic\{number}"));
            paths.push((path, format!("asset-{number}")));
            images.push(format!("asset-{number}"));
        }
        let library = PathLibrary::of(
            &paths
                .iter()
                .map(|(path, asset)| (path.as_str(), asset.as_str()))
                .collect::<Vec<_>>(),
        );
        let source = FakeSource::with(
            &images
                .iter()
                .map(|id| (id.as_str(), JPEG))
                .collect::<Vec<_>>(),
        );

        let served = serve_misses(
            &cache,
            &library,
            &source,
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &NoNotes,
        )
        .await;

        assert_eq!(served.stored, 12);
        assert_eq!(library.listings(), 1, "one listing serves all twelve");
    }

    #[tokio::test]
    async fn requests_in_different_directories_are_each_served_from_their_own_listing() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"Albums/A/a.jpg", r"C:\Lymic\a");
        cache.record_miss(b"Albums/B/b.jpg", r"C:\Lymic\b");
        let library = PathLibrary::of(&[("Albums/A/a.jpg", "1"), ("Albums/B/b.jpg", "2")]);
        let source = FakeSource::with(&[("1", JPEG), ("2", JPEG)]);

        let served = serve_misses(
            &cache,
            &library,
            &source,
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &NoNotes,
        )
        .await;

        assert_eq!(served.stored, 2);
        assert_eq!(library.listings(), 2);
    }

    #[tokio::test]
    async fn a_request_for_a_name_the_directory_does_not_hold_is_logged_with_its_path_and_reason() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"All Photos/2016/04 April/gone.jpg", r"C:\Lymic\gone.jpg");
        let library = PathLibrary::of(&[("All Photos/2016/04 April/other.jpg", "1")]);
        let notes = RecordedNotes::default();

        serve_misses(
            &cache,
            &library,
            &FakeSource::with(&[]),
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &notes,
        )
        .await;

        let text = notes.text();
        assert!(text.contains("All Photos/2016/04 April/gone.jpg"), "{text}");
        assert!(text.contains("no file of that name"), "{text}");
    }

    #[tokio::test]
    async fn a_request_whose_directory_cannot_be_listed_is_logged_with_the_reason() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"All Photos/2016/04 April/a.jpg", r"C:\Lymic\a.jpg");
        let library = PathLibrary::of(&[]);
        let notes = RecordedNotes::default();

        serve_misses(
            &cache,
            &library,
            &FakeSource::with(&[]),
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &notes,
        )
        .await;

        let text = notes.text();
        assert!(text.contains("All Photos/2016/04 April"), "{text}");
        assert!(text.contains("NotFound"), "{text}");
    }

    #[tokio::test]
    async fn a_request_given_up_after_three_rounds_is_logged() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"a.jpg", r"C:\Lymic\a.jpg");
        let library = PathLibrary::of(&[("a.jpg", "gone")]);
        let notes = RecordedNotes::default();
        let mut queue = MissQueue::default();

        for _ in 0..3 {
            serve_misses(
                &cache,
                &library,
                &FakeSource::with(&[]),
                &RecordingRefresh::default(),
                &mut queue,
                FAST,
                &notes,
            )
            .await;
        }

        let text = notes.text();
        assert!(text.contains("gave up on 'a.jpg'"), "{text}");
        assert!(text.contains("no thumbnail from Immich"), "{text}");
    }

    #[tokio::test]
    async fn the_thumbnails_cached_for_a_directory_are_counted_in_the_log() {
        let dir = TempDir::new();
        let cache = dir.cache();
        cache.record_miss(b"Albums/A/a.jpg", r"C:\Lymic\a");
        let library = PathLibrary::of(&[("Albums/A/a.jpg", "1")]);
        let notes = RecordedNotes::default();

        serve_misses(
            &cache,
            &library,
            &FakeSource::with(&[("1", JPEG)]),
            &RecordingRefresh::default(),
            &mut MissQueue::default(),
            FAST,
            &notes,
        )
        .await;

        let text = notes.text();
        assert!(text.contains("'Albums/A'"), "{text}");
        assert!(text.contains("1 cached"), "{text}");
    }
}
