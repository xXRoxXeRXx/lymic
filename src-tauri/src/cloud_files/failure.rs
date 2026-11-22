//! Why serving a Cloud Files request failed, and which Cloud Files error the platform is told.
//!
//! The platform turns the status we complete a request with into the error the reading program
//! sees, so the mapping is the whole user-visible behaviour of a failure. Retries, the in-app notice
//! and the re-login offer are separate work (#281); this is the one place they hook into.

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
}

/// The Cloud Files error a request is completed with. The adapter maps each to its `NTSTATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloudStatus {
    /// `STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`
    NetworkUnavailable,
    /// `STATUS_CLOUD_FILE_AUTHENTICATION_FAILED`
    AuthenticationFailed,
    /// `STATUS_CLOUD_FILE_INVALID_REQUEST`
    InvalidRequest,
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
            FailureKind::NotFound => CloudStatus::InvalidRequest,
            FailureKind::Failed => CloudStatus::Unsuccessful,
        }
    }

    /// The failure of a projection call. Immich errors reach it as text, so a status code in the
    /// text decides; text without one is a transport failure, which means Immich is unreachable.
    pub(crate) fn from_library(error: VirtualLibraryError) -> Self {
        match error {
            VirtualLibraryError::NotFound
            | VirtualLibraryError::NotADirectory
            | VirtualLibraryError::NotAFile => Self::new(FailureKind::NotFound, "no such item"),
            VirtualLibraryError::Remote(message) => {
                let kind = if message == "Not authenticated"
                    || message.contains("status 401")
                    || message.contains("status 403")
                {
                    FailureKind::Unauthorized
                } else if message.contains("status 404") {
                    FailureKind::NotFound
                } else if message.contains("status ") {
                    FailureKind::Failed
                } else {
                    FailureKind::Offline
                };
                Self::new(kind, message)
            }
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
            (FailureKind::NotFound, CloudStatus::InvalidRequest),
            (FailureKind::Failed, CloudStatus::Unsuccessful),
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
    fn an_immich_error_is_classified_by_the_status_it_reports() {
        let kind = |message: &str| {
            Failure::from_library(VirtualLibraryError::Remote(message.to_string())).kind
        };

        assert_eq!(
            kind("Failed to list: request failed with status 401 Unauthorized"),
            FailureKind::Unauthorized
        );
        assert_eq!(
            kind("request failed with status 403 Forbidden"),
            FailureKind::Unauthorized
        );
        assert_eq!(kind("Not authenticated"), FailureKind::Unauthorized);
        assert_eq!(
            kind("request failed with status 404 Not Found"),
            FailureKind::NotFound
        );
        assert_eq!(
            kind("request failed with status 500 Internal Server Error"),
            FailureKind::Failed
        );
        assert_eq!(
            kind("Failed to search: error sending request for url"),
            FailureKind::Offline
        );
    }
}
