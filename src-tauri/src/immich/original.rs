//! Streams the full original of a Remote Asset, for the Virtual Library's Hydration (ADR-0003).
//!
//! Unlike `download_asset` it writes no file: the caller reads the body chunk by chunk and hands
//! each chunk to the platform. It never asks for a preview variant and never for a byte range, so
//! the whole original comes back, and it only reads: no Immich write is involved.

use super::client::ImmichClient;
use super::remote_failure::{send_bounded, RemoteFailure, RemoteFailureKind};
use bytes::Bytes;
use reqwest::{Method, Response};
use std::time::Duration;

/// How long Explorer may wait on Immich: a read that cannot make progress fails instead of
/// hanging, and the reader is told so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OriginalTimeouts {
    /// Connecting and waiting for the response headers.
    pub response: Duration,
    /// Waiting for the next piece of an original that has started to arrive. The whole download
    /// has no deadline of its own: a large original on a slow link is fine while bytes keep coming.
    pub stall: Duration,
}

impl Default for OriginalTimeouts {
    fn default() -> Self {
        Self {
            response: Duration::from_secs(15),
            stall: Duration::from_secs(30),
        }
    }
}

/// The body of an original being read.
pub(crate) struct AssetOriginal {
    response: Response,
    stall: Duration,
}

impl AssetOriginal {
    /// The next piece of the original, or `None` at its end.
    pub(crate) async fn next_chunk(&mut self) -> Result<Option<Bytes>, RemoteFailure> {
        match tokio::time::timeout(self.stall, self.response.chunk()).await {
            Ok(Ok(chunk)) => Ok(chunk),
            Ok(Err(error)) => Err(RemoteFailure::unavailable(format!(
                "Failed to read asset original: {error}"
            ))),
            Err(_) => Err(RemoteFailure::unavailable(format!(
                "Failed to read asset original: no data for {} seconds",
                self.stall.as_secs().max(1)
            ))),
        }
    }
}

impl ImmichClient {
    /// Starts reading the full original of `asset_id`.
    pub(crate) async fn open_asset_original(
        &self,
        asset_id: &str,
    ) -> Result<AssetOriginal, RemoteFailure> {
        self.open_asset_original_within(asset_id, OriginalTimeouts::default())
            .await
    }

    pub(crate) async fn open_asset_original_within(
        &self,
        asset_id: &str,
        timeouts: OriginalTimeouts,
    ) -> Result<AssetOriginal, RemoteFailure> {
        Self::validate_path_segment(asset_id, "asset ID")
            .map_err(|message| RemoteFailure::new(RemoteFailureKind::NotFound, message))?;
        let response = send_bounded(
            self.request(Method::GET, &format!("assets/{asset_id}/original")),
            "Failed to download asset original",
            timeouts.response,
        )
        .await?;
        Ok(AssetOriginal {
            response,
            stall: timeouts.stall,
        })
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
            ("401 Unauthorized", RemoteFailureKind::Unauthorized),
            ("403 Forbidden", RemoteFailureKind::Unauthorized),
            ("404 Not Found", RemoteFailureKind::NotFound),
            ("500 Internal Server Error", RemoteFailureKind::Rejected),
            ("503 Service Unavailable", RemoteFailureKind::Unavailable),
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

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
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

        assert_eq!(error.kind, RemoteFailureKind::NotFound);
    }
    fn quick() -> OriginalTimeouts {
        OriginalTimeouts {
            response: Duration::from_millis(150),
            stall: Duration::from_millis(150),
        }
    }

    #[tokio::test]
    async fn a_server_that_never_answers_fails_the_open_instead_of_hanging() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hold = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let error = match test_client(url, "key")
            .open_asset_original_within("a", quick())
            .await
        {
            Ok(_) => panic!("a silent server must fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
        hold.abort();
    }

    #[tokio::test]
    async fn a_download_that_stops_arriving_fails_the_read_instead_of_hanging() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        // Announces 100 bytes, sends 5, then goes quiet with the connection open.
        let hold = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK
content-length: 100

hello",
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let mut original = test_client(url, "key")
            .open_asset_original_within("a", quick())
            .await
            .unwrap();

        assert_eq!(original.next_chunk().await.unwrap().unwrap(), "hello");
        let error = original.next_chunk().await.unwrap_err();

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
        assert!(error.message.contains("no data"), "{}", error.message);
        hold.abort();
    }
}
