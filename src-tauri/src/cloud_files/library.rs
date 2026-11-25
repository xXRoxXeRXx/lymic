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
use std::future::Future;

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
        VirtualLibrary::new(&client)
            .list(path)
            .await
            .map_err(|error| redacted(&session, error))
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
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
