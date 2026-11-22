//! Streams the full original of a Remote Asset, for the Virtual Library's Hydration (ADR-0003).
//!
//! Unlike `download_asset` it writes no file: the caller reads the body chunk by chunk and hands
//! each chunk to the platform. It never asks for a preview variant and never for a byte range, so
//! the whole original comes back, and it only reads: no Immich write is involved.

use super::client::ImmichClient;
use super::response::response_status_error;
use bytes::Bytes;
use reqwest::{Method, Response, StatusCode};

/// Why an original could not be read, in the terms Hydration needs to pick a Cloud Files error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginalFailureKind {
    /// 401 or 403: the API key is no longer accepted.
    Unauthorized,
    /// 404: the Remote Asset no longer exists (or is not visible to this key).
    NotFound,
    /// The server could not be reached, or answered with an error or a broken body.
    Unavailable,
}

/// A failed original read. `message` never carries credentials or a response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginalError {
    pub kind: OriginalFailureKind,
    pub message: String,
}

impl OriginalFailureKind {
    fn of_status(status: StatusCode) -> Self {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::Unauthorized,
            StatusCode::NOT_FOUND => Self::NotFound,
            _ => Self::Unavailable,
        }
    }
}

/// The body of an original being read.
pub(crate) struct AssetOriginal {
    response: Response,
}

impl AssetOriginal {
    /// The next piece of the original, or `None` at its end.
    pub(crate) async fn next_chunk(&mut self) -> Result<Option<Bytes>, OriginalError> {
        self.response.chunk().await.map_err(|error| OriginalError {
            kind: OriginalFailureKind::Unavailable,
            message: format!("Failed to read asset original: {error}"),
        })
    }
}

impl ImmichClient {
    /// Starts reading the full original of `asset_id`.
    pub(crate) async fn open_asset_original(
        &self,
        asset_id: &str,
    ) -> Result<AssetOriginal, OriginalError> {
        Self::validate_path_segment(asset_id, "asset ID").map_err(|message| OriginalError {
            kind: OriginalFailureKind::NotFound,
            message,
        })?;
        let response = self
            .request(Method::GET, &format!("assets/{asset_id}/original"))
            .send()
            .await
            .map_err(|error| OriginalError {
                kind: OriginalFailureKind::Unavailable,
                message: format!("Failed to download asset original: {error}"),
            })?;
        if !response.status().is_success() {
            let kind = OriginalFailureKind::of_status(response.status());
            let detail = response_status_error(response)
                .await
                .unwrap_or_else(|error| error);
            return Err(OriginalError {
                kind,
                message: format!("Failed to download asset original: {detail}"),
            });
        }
        Ok(AssetOriginal { response })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::test_support::{serve_once, test_client};

    #[tokio::test]
    async fn streams_the_full_original_from_the_original_endpoint_without_a_range() {
        let (url, request) = serve_once("200 OK", "the-original-bytes").await;
        let client = test_client(url, "key");

        let mut original = client.open_asset_original("asset-1").await.unwrap();

        let mut body = Vec::new();
        while let Some(chunk) = original.next_chunk().await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        assert_eq!(body, b"the-original-bytes");
        let request = request.await.unwrap().to_lowercase();
        assert!(
            request.starts_with("get /api/assets/asset-1/original http/1.1"),
            "{request}"
        );
        assert!(!request.contains("range:"), "{request}");
    }

    #[tokio::test]
    async fn classifies_the_status_of_a_refused_original() {
        for (status, kind) in [
            ("401 Unauthorized", OriginalFailureKind::Unauthorized),
            ("403 Forbidden", OriginalFailureKind::Unauthorized),
            ("404 Not Found", OriginalFailureKind::NotFound),
            (
                "500 Internal Server Error",
                OriginalFailureKind::Unavailable,
            ),
        ] {
            let (url, _request) = serve_once(status, "secret server detail").await;

            let error = match test_client(url, "key").open_asset_original("a").await {
                Ok(_) => panic!("{status} must fail"),
                Err(error) => error,
            };

            assert_eq!(error.kind, kind, "{status}");
            assert!(error.message.contains(&status[..3]), "{}", error.message);
            assert!(
                !error.message.contains("secret server detail"),
                "never forwards a response body: {}",
                error.message
            );
        }
    }

    #[tokio::test]
    async fn an_unreachable_server_is_unavailable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let error = match test_client(url, "key").open_asset_original("a").await {
            Ok(_) => panic!("an unreachable server must fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind, OriginalFailureKind::Unavailable);
    }

    #[tokio::test]
    async fn an_asset_id_that_is_not_a_path_segment_never_reaches_the_server() {
        let error = match test_client("http://127.0.0.1:1".to_string(), "key")
            .open_asset_original("../etc")
            .await
        {
            Ok(_) => panic!("must be refused"),
            Err(error) => error,
        };

        assert_eq!(error.kind, OriginalFailureKind::NotFound);
    }
}
