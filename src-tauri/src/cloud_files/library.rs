//! What the adapter needs from the Immich side, as small traits, and the one real implementation.
//!
//! The adapter only ever asks three read-only questions: what is in a directory, which Remote
//! Asset is this file (both answered by the projection in `virtual_library`, which owns every
//! naming, path and date rule), and the bytes of an original. Keeping them behind traits lets the
//! orchestration (population, hydration) be tested without Windows and without a server.

use super::failure::{Failure, FailureKind};
use crate::immich::{
    AssetOriginal, ImmichClient, ImmichSession, OriginalError, OriginalFailureKind, RemoteAssetId,
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
}

/// The size of a Remote Asset's original, for entries the listing could not size.
pub(super) trait SizeLookup: Sync {
    fn size_of(&self, asset: &RemoteAssetId) -> impl Future<Output = Option<u64>> + Send;
}

/// The original of a Remote Asset, read as a stream.
pub(super) trait OriginalSource: Sync {
    type Stream: OriginalStream;

    fn open(
        &self,
        asset: &RemoteAssetId,
    ) -> impl Future<Output = Result<Self::Stream, Failure>> + Send;
}

/// The body of an original being read.
pub(super) trait OriginalStream: Send {
    /// The next piece, or `None` at the end.
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Bytes>, Failure>> + Send;
}

/// Immich as the signed-in user: every call opens the session from the stored credentials (the
/// session is cached, so this is cheap) and so follows a re-login without a restart. Errors are
/// redacted of credentials before they leave.
pub(super) struct StoredCredentialsImmich;

impl StoredCredentialsImmich {
    fn session() -> Result<ImmichSession, Failure> {
        ImmichSession::from_stored_credentials()
            .map_err(|message| Failure::from_library(VirtualLibraryError::Remote(message)))
    }
}

impl Library for StoredCredentialsImmich {
    async fn list(&self, path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
        let session = open_session()?;
        let client = session.client();
        VirtualLibrary::new(&client)
            .list(path)
            .await
            .map_err(|error| redacted(&session, error))
    }

    async fn resolve(&self, path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
        let session = open_session()?;
        let client = session.client();
        VirtualLibrary::new(&client)
            .resolve(path)
            .await
            .map_err(|error| redacted(&session, error))
    }
}

fn open_session() -> Result<ImmichSession, VirtualLibraryError> {
    ImmichSession::from_stored_credentials().map_err(VirtualLibraryError::Remote)
}

fn redacted(session: &ImmichSession, error: VirtualLibraryError) -> VirtualLibraryError {
    match error {
        VirtualLibraryError::Remote(message) => {
            VirtualLibraryError::Remote(session.credentials().redact(&message))
        }
        other => other,
    }
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

    async fn open(&self, asset: &RemoteAssetId) -> Result<ImmichOriginal, Failure> {
        let session = Self::session()?;
        let client = session.client();
        match client.open_asset_original(asset.as_str()).await {
            Ok(body) => Ok(ImmichOriginal { body, session }),
            Err(error) => Err(original_failure(&session, error)),
        }
    }
}

impl OriginalStream for ImmichOriginal {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, Failure> {
        match self.body.next_chunk().await {
            Ok(chunk) => Ok(chunk),
            Err(error) => Err(original_failure(&self.session, error)),
        }
    }
}

fn original_failure(session: &ImmichSession, error: OriginalError) -> Failure {
    let kind = match error.kind {
        OriginalFailureKind::Unauthorized => FailureKind::Unauthorized,
        OriginalFailureKind::NotFound => FailureKind::NotFound,
        OriginalFailureKind::Unavailable => FailureKind::Offline,
    };
    Failure::new(kind, session.credentials().redact(&error.message))
}
