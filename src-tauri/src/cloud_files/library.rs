//! What the adapter needs from the Immich side, as small traits, and the one real implementation.
//!
//! The adapter only ever asks three read-only questions: what is in a directory, which Remote
//! Asset is this file (both answered by the projection in `virtual_library`, which owns every
//! naming, path and date rule), and the bytes of an original. Keeping them behind traits lets the
//! orchestration (population, hydration) be tested without Windows and without a server.

use super::failure::Failure;
use crate::immich::{
    AssetOriginal, ImmichClient, ImmichSession, RemoteAssetId, RemoteFailure, RemoteFailureKind,
};
use crate::virtual_library::{Entry, VirtualLibrary, VirtualLibraryError};
use bytes::Bytes;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The Virtual Library tree. Paths are relative to the root, `/` separated, `""` is the root.
pub(super) trait Library: Sync {
    fn list(
        &self,
        path: &str,
    ) -> impl Future<Output = Result<Vec<Entry>, VirtualLibraryError>> + Send;

    fn resolve(
        &self,
        path: &str,
    ) -> impl Future<Output = Result<RemoteAssetId, VirtualLibraryError>> + Send;

    /// Why nothing was found at `path`, for the diagnostic log (what Immich reports there).
    fn explain_missing(&self, _path: &str) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

/// The size of a Remote Asset's original, for entries the listing could not size.
pub(super) trait SizeLookup: Sync {
    fn size_of(&self, asset: &RemoteAssetId) -> impl Future<Output = Option<u64>> + Send;
}

/// The original of a Remote Asset, read as a stream.
pub(super) trait OriginalSource: Sync {
    type Stream: OriginalStream;

    /// Starts reading at byte `from`: the stream may begin there ([`OriginalStream::starts_at`]) or,
    /// when the server cannot serve a range, at byte 0.
    fn open(
        &self,
        asset: &RemoteAssetId,
        from: u64,
    ) -> impl Future<Output = Result<Self::Stream, Failure>> + Send;
}

/// The body of an original being read.
pub(super) trait OriginalStream: Send {
    /// The byte of the original the body begins at.
    fn starts_at(&self) -> u64 {
        0
    }

    /// The length the server announced for the whole original, when it did.
    fn content_length(&self) -> Option<u64> {
        None
    }

    /// The next piece, or `None` at the end.
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Bytes>, Failure>> + Send;
}

/// How long a listed file keeps pointing at its Remote Asset without a new listing.
const ASSET_PATH_TTL: Duration = Duration::from_secs(10 * 60);
/// More files than this are forgotten at once, so the table cannot grow without bound.
const ASSET_PATH_LIMIT: usize = 200_000;

/// Which Remote Asset each file of an already listed directory is. Resolving a path means listing
/// its whole directory (hundreds of entries), which takes about as long as a reader is willing to
/// wait for its first bytes: a read right after a listing, which is how every file is opened,
/// finds its asset here instead.
#[derive(Default)]
pub(super) struct AssetPaths {
    files: Mutex<HashMap<String, (RemoteAssetId, Instant)>>,
}

impl AssetPaths {
    /// Remembers the files among `entries`, the children of the directory `dir`.
    pub(super) fn remember(&self, dir: &str, entries: &[Entry], now: Instant) {
        let Ok(mut files) = self.files.lock() else {
            return;
        };
        if files.len() > ASSET_PATH_LIMIT {
            files.clear();
        }
        for entry in entries {
            if let Some(asset) = &entry.remote_asset {
                files.insert(join(dir, &entry.name), (asset.clone(), now));
            }
        }
    }

    /// The asset of the file at `path`, when it was listed within [`ASSET_PATH_TTL`].
    pub(super) fn lookup(&self, path: &str, now: Instant) -> Option<RemoteAssetId> {
        let files = self.files.lock().ok()?;
        let (asset, listed) = files.get(path)?;
        (now.saturating_duration_since(*listed) < ASSET_PATH_TTL).then(|| asset.clone())
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

static ASSET_PATHS: std::sync::LazyLock<AssetPaths> = std::sync::LazyLock::new(AssetPaths::default);

/// Immich as the signed-in user: every call opens the session from the stored credentials (the
/// session is cached, so this is cheap) and so follows a re-login without a restart. Errors are
/// redacted of credentials before they leave.
#[derive(Clone, Copy)]
pub(super) struct StoredCredentialsImmich;

impl StoredCredentialsImmich {
    /// The session for the stored credentials. No credentials, or credentials that cannot be read,
    /// mean the user has to sign in again.
    pub(super) fn session() -> Result<ImmichSession, RemoteFailure> {
        let credentials = crate::auth::get_credentials()
            .map_err(|_| {
                RemoteFailure::new(
                    RemoteFailureKind::Unauthorized,
                    "the stored credentials could not be read",
                )
            })?
            .ok_or_else(|| RemoteFailure::new(RemoteFailureKind::Unauthorized, "not signed in"))?;
        ImmichSession::from_credentials(credentials).map_err(RemoteFailure::unavailable)
    }
}

impl Library for StoredCredentialsImmich {
    async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
        let session = Self::session().map_err(VirtualLibraryError::Remote)?;
        let client = session.client();
        let entries = VirtualLibrary::new(&client)
            .list(path)
            .await
            .map_err(|error| redacted(&session, error))?;
        ASSET_PATHS.remember(path, &entries, Instant::now());
        Ok(entries)
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        if let Some(asset) = ASSET_PATHS.lookup(path, Instant::now()) {
            return Ok(asset);
        }
        let session = Self::session().map_err(VirtualLibraryError::Remote)?;
        let client = session.client();
        VirtualLibrary::new(&client)
            .resolve(path)
            .await
            .map_err(|error| redacted(&session, error))
    }

    async fn explain_missing(&self, path: &str) -> Option<String> {
        let session = Self::session().ok()?;
        let client = session.client();
        let why = VirtualLibrary::new(&client).explain_missing(path).await?;
        Some(session.credentials().redact(&why))
    }
}

fn redacted(session: &ImmichSession, error: VirtualLibraryError) -> VirtualLibraryError {
    match error {
        VirtualLibraryError::Remote(failure) => {
            VirtualLibraryError::Remote(redact_failure(session, failure))
        }
        other => other,
    }
}

fn redact_failure(session: &ImmichSession, failure: RemoteFailure) -> RemoteFailure {
    RemoteFailure::new(failure.kind, session.credentials().redact(&failure.message))
}

impl SizeLookup for StoredCredentialsImmich {
    async fn size_of(&self, asset: &RemoteAssetId) -> Option<u64> {
        let session = Self::session().ok()?;
        let client: std::sync::Arc<ImmichClient> = session.client();
        client
            .get_asset_details(asset.as_str())
            .await
            .ok()
            .and_then(|details| details.file_size)
    }
}

/// An original being read with the credentials it was opened with, to redact its errors.
pub(super) struct ImmichOriginal {
    body: AssetOriginal,
    session: ImmichSession,
}

impl OriginalSource for StoredCredentialsImmich {
    type Stream = ImmichOriginal;

    async fn open(&self, asset: &RemoteAssetId, from: u64) -> Result<ImmichOriginal, Failure> {
        let session = Self::session().map_err(Failure::from_remote)?;
        let client = session.client();
        match client.open_asset_original_at(asset.as_str(), from).await {
            Ok(body) => Ok(ImmichOriginal { body, session }),
            Err(error) => Err(Failure::from_remote(redact_failure(&session, error))),
        }
    }
}

impl OriginalStream for ImmichOriginal {
    fn starts_at(&self) -> u64 {
        self.body.starts_at()
    }

    fn content_length(&self) -> Option<u64> {
        self.body.content_length()
    }

    async fn next_chunk(&mut self) -> Result<Option<Bytes>, Failure> {
        match self.body.next_chunk().await {
            Ok(chunk) => Ok(chunk),
            Err(error) => Err(Failure::from_remote(redact_failure(&self.session, error))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_library::EntryKind;

    fn file(name: &str, asset: &str) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::File,
            remote_asset: Some(RemoteAssetId::new(asset)),
            size: Some(1),
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

    #[test]
    fn a_listed_file_resolves_without_a_new_listing_and_directories_do_not() {
        let paths = AssetPaths::default();
        let now = Instant::now();
        paths.remember(
            "All Photos/2024/03 March",
            &[file("IMG_1.jpg", "a1"), directory("sub")],
            now,
        );

        assert_eq!(
            paths.lookup("All Photos/2024/03 March/IMG_1.jpg", now),
            Some(RemoteAssetId::new("a1"))
        );
        assert_eq!(paths.lookup("All Photos/2024/03 March/sub", now), None);
        assert_eq!(
            paths.lookup("All Photos/2024/03 March/other.jpg", now),
            None
        );
    }

    #[test]
    fn a_file_in_the_root_has_no_leading_separator() {
        let paths = AssetPaths::default();
        let now = Instant::now();
        paths.remember("", &[file("a.jpg", "a1")], now);

        assert_eq!(paths.lookup("a.jpg", now), Some(RemoteAssetId::new("a1")));
    }

    #[test]
    fn a_file_is_forgotten_after_the_time_to_live_and_a_new_listing_replaces_it() {
        let paths = AssetPaths::default();
        let listed = Instant::now();
        paths.remember("d", &[file("a.jpg", "old")], listed);

        let later = listed + ASSET_PATH_TTL + Duration::from_secs(1);
        assert_eq!(paths.lookup("d/a.jpg", later), None);

        paths.remember("d", &[file("a.jpg", "new")], later);
        assert_eq!(
            paths.lookup("d/a.jpg", later),
            Some(RemoteAssetId::new("new"))
        );
    }
}
