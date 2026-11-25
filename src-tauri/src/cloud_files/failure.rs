//! Why serving a Cloud Files request failed, and which Cloud Files error the platform is told.
//!
//! The platform turns the status we complete a request with into the error the reading program
//! sees, so the mapping is the whole user-visible behaviour of a failure. The in-app notice and the
//! re-login offer hang off the same classification (`notice`, #281). Nothing here retries: a failed
//! request fails the read once and promptly.

use crate::immich::{RemoteFailure, RemoteFailureKind};
use crate::virtual_library::VirtualLibraryError;

/// What went wrong, in the terms that decide the Cloud Files error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureKind {
    /// Immich cannot be reached.
    Offline,
    /// The stored API key is missing or no longer accepted.
    Unauthorized,
    /// The path or the Remote Asset does not exist (any more).
    NotFound,
    /// Anything else, including a body that does not match the announced size.
    Failed,
    /// The platform refused or ended a transfer (the reader went away). Nothing for the user to
    /// fix and nothing to tell them.
    Platform,
}

/// The Cloud Files error a request is completed with. The adapter maps each to its `NTSTATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloudStatus {
    /// `STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`
    NetworkUnavailable,
    /// `STATUS_CLOUD_FILE_AUTHENTICATION_FAILED`
    AuthenticationFailed,
    /// `STATUS_OBJECT_NAME_NOT_FOUND`: the item is gone, so the reader sees "file not found".
    NotFound,
    /// `STATUS_CLOUD_FILE_UNSUCCESSFUL`
    Unsuccessful,
}

/// A failed request. `message` never carries credentials or a response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

impl Failure {
    pub(crate) fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn status(&self) -> CloudStatus {
        match self.kind {
            FailureKind::Offline => CloudStatus::NetworkUnavailable,
            FailureKind::Unauthorized => CloudStatus::AuthenticationFailed,
            FailureKind::NotFound => CloudStatus::NotFound,
            FailureKind::Failed | FailureKind::Platform => CloudStatus::Unsuccessful,
        }
    }

    /// The failure of a request to Immich, classified by the kind the Immich client decided where
    /// it knew the HTTP status. A server that could not be reached or did not answer (or a gateway
    /// in front of it said so) is `Offline`; one that answered with another error is `Failed`.
    pub(crate) fn from_remote(failure: RemoteFailure) -> Self {
        let kind = match failure.kind {
            RemoteFailureKind::Unauthorized => FailureKind::Unauthorized,
            RemoteFailureKind::NotFound => FailureKind::NotFound,
            RemoteFailureKind::Unavailable => FailureKind::Offline,
            RemoteFailureKind::Rejected => FailureKind::Failed,
        };
        Self::new(kind, failure.message)
    }

    /// The failure of a projection call.
    pub(crate) fn from_library(error: VirtualLibraryError) -> Self {
        match error {
            VirtualLibraryError::NotFound
            | VirtualLibraryError::NotADirectory
            | VirtualLibraryError::NotAFile => Self::new(FailureKind::NotFound, "no such item"),
            VirtualLibraryError::Remote(failure) => Self::from_remote(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_completes_the_request_with_its_own_cloud_files_error() {
        for (kind, status) in [
            (FailureKind::Offline, CloudStatus::NetworkUnavailable),
            (FailureKind::Unauthorized, CloudStatus::AuthenticationFailed),
            (FailureKind::NotFound, CloudStatus::NotFound),
            (FailureKind::Failed, CloudStatus::Unsuccessful),
            (FailureKind::Platform, CloudStatus::Unsuccessful),
        ] {
            assert_eq!(Failure::new(kind, "x").status(), status, "{kind:?}");
        }
    }

    #[test]
    fn a_missing_path_or_wrong_kind_of_item_is_not_found() {
        for error in [
            VirtualLibraryError::NotFound,
            VirtualLibraryError::NotADirectory,
            VirtualLibraryError::NotAFile,
        ] {
            assert_eq!(Failure::from_library(error).kind, FailureKind::NotFound);
        }
    }

    #[test]
    fn an_immich_failure_is_classified_by_its_typed_kind_whatever_its_text_says() {
        for (remote, kind) in [
            (RemoteFailureKind::Unauthorized, FailureKind::Unauthorized),
            (RemoteFailureKind::NotFound, FailureKind::NotFound),
            (RemoteFailureKind::Unavailable, FailureKind::Offline),
            (RemoteFailureKind::Rejected, FailureKind::Failed),
        ] {
            // Text that would have fooled a text match must not matter.
            let failure = RemoteFailure::new(remote, "status 404 status 401 Not authenticated");

            assert_eq!(
                Failure::from_library(VirtualLibraryError::Remote(failure.clone())).kind,
                kind,
                "{remote:?}"
            );
            let converted = Failure::from_remote(failure.clone());
            assert_eq!(converted.kind, kind, "{remote:?}");
            assert_eq!(converted.message, failure.message);
        }
    }
}
