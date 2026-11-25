//! Streams the full original of a Remote Asset, for the Virtual Library's Hydration (ADR-0003).
//!
//! Unlike `download_asset` it writes no file: the caller reads the body chunk by chunk and hands
//! each chunk to the platform. It never asks for a preview variant. It starts at byte 0 unless the
//! caller needs a later start, which it asks for with a `Range` request; a server that does not
//! honour it (anything but a matching `206`) is answered by reading the whole original instead. It
//! only reads: no Immich write is involved.

use super::client::ImmichClient;
use super::remote_failure::{
    send_bounded, send_bounded_allowing, RemoteFailure, RemoteFailureKind,
};
use bytes::Bytes;
use reqwest::{Method, Response, StatusCode};
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
    /// The byte of the original the body begins at: 0, or the offset a range request was granted.
    starts_at: u64,
    /// The length of the whole original, when the server said.
    total: Option<u64>,
}

impl AssetOriginal {
    /// The length of the whole original, when the server announced it (`Content-Length` of a full
    /// answer, the total of `Content-Range` of a partial one; not for a compressed or chunked
    /// body).
    pub(crate) fn content_length(&self) -> Option<u64> {
        self.total
    }

    /// The byte of the original the body begins at. It is the offset asked for when the server
    /// answered the range request with `206`, and 0 when it sent the whole original.
    pub(crate) fn starts_at(&self) -> u64 {
        self.starts_at
    }

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
    /// Starts reading the original of `asset_id` at byte `offset`, 0 for the whole original (see [`Self::open_asset_original_from`]).
    pub(crate) async fn open_asset_original_at(
        &self,
        asset_id: &str,
        offset: u64,
    ) -> Result<AssetOriginal, RemoteFailure> {
        self.open_asset_original_from(asset_id, offset, OriginalTimeouts::default())
            .await
    }

    /// Starts reading the original of `asset_id` at byte `offset`. For an offset above 0 it asks
    /// for `Range: bytes=<offset>-`; the answer decides what the body is
    /// ([`AssetOriginal::starts_at`]): a `206` that begins at `offset` serves only the rest, and
    /// anything else (`200` because the server or a proxy ignores ranges, `416` because the size
    /// changed, a `206` that begins elsewhere) makes this read the whole original from byte 0, so a
    /// server without range support costs only the bytes the caller then skips.
    pub(crate) async fn open_asset_original_from(
        &self,
        asset_id: &str,
        offset: u64,
        timeouts: OriginalTimeouts,
    ) -> Result<AssetOriginal, RemoteFailure> {
        Self::validate_path_segment(asset_id, "asset ID")
            .map_err(|message| RemoteFailure::new(RemoteFailureKind::NotFound, message))?;
        let path = format!("assets/{asset_id}/original");
        let failure = "Failed to download asset original";
        if offset > 0 {
            let response = send_bounded_allowing(
                self.request(Method::GET, &path)
                    .header(reqwest::header::RANGE, format!("bytes={offset}-")),
                failure,
                timeouts.response,
                &[StatusCode::RANGE_NOT_SATISFIABLE],
            )
            .await?;
            if response.status() == StatusCode::PARTIAL_CONTENT {
                if let Some(total) = granted_range(&response, offset) {
                    return Ok(AssetOriginal {
                        response,
                        stall: timeouts.stall,
                        starts_at: offset,
                        total: Some(total),
                    });
                }
            } else if response.status() == StatusCode::OK {
                return Ok(AssetOriginal::whole(response, timeouts.stall));
            }
            // Not what was asked for: the whole original, with the answer above dropped unread.
            drop(response);
        }
        let response =
            send_bounded(self.request(Method::GET, &path), failure, timeouts.response).await?;
        Ok(AssetOriginal::whole(response, timeouts.stall))
    }
}

impl AssetOriginal {
    fn whole(response: Response, stall: Duration) -> Self {
        let total = response.content_length();
        Self {
            response,
            stall,
            starts_at: 0,
            total,
        }
    }
}

/// The total length of the original when `response` is a `206` that begins exactly at `offset`
/// (`Content-Range: bytes <offset>-<end>/<total>`); `None` for anything else, including an
/// unknown total (`*`), which cannot be checked against the placeholder.
fn granted_range(response: &Response, offset: u64) -> Option<u64> {
    let header = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    let (range, total) = header.strip_prefix("bytes ")?.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    (start.trim().parse::<u64>().ok()? == offset).then_some(())?;
    total.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::test_support::{serve_once, test_client};

    #[tokio::test]
    async fn streams_the_full_original_from_the_original_endpoint_without_a_range() {
        let (url, request) = serve_once("200 OK", "the-original-bytes").await;
        let client = test_client(url, "key");

        let mut original = client.open_asset_original_at("asset-1", 0).await.unwrap();

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

            let error = match test_client(url, "key").open_asset_original_at("a", 0).await {
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

        let error = match test_client(url, "key").open_asset_original_at("a", 0).await {
            Ok(_) => panic!("an unreachable server must fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
    }

    #[tokio::test]
    async fn an_asset_id_that_is_not_a_path_segment_never_reaches_the_server() {
        let error = match test_client("http://127.0.0.1:1".to_string(), "key")
            .open_asset_original_at("../etc", 0)
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

    /// A fake Immich original endpoint over a raw socket, which can see the `Range` header. It
    /// answers `GET .../original` with `body`; `mode` decides what it does with a range request.
    #[derive(Clone, Copy)]
    enum RangeMode {
        /// Express `sendFile`, which is what Immich uses: `206` with `Content-Range`.
        Honours,
        /// A proxy or server that ignores `Range` and sends the whole body with `200`.
        Ignores,
        /// `416` for any range.
        NotSatisfiable,
        /// `206`, but starting somewhere other than where it was asked.
        WrongStart,
    }

    /// Returns the URL and the `Range` header of each request received (`None` when absent).
    async fn serve_original(
        body: Vec<u8>,
        mode: RangeMode,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_lowercase();
                let range = request
                    .lines()
                    .find_map(|line| line.strip_prefix("range: ").map(str::to_string));
                log.lock().unwrap().push(range.clone());
                let total = body.len();
                let start = range
                    .as_deref()
                    .and_then(|range| range.strip_prefix("bytes="))
                    .and_then(|range| range.strip_suffix('-'))
                    .and_then(|start| start.parse::<usize>().ok());
                let response: Vec<u8> = match (start, mode) {
                    (Some(start), RangeMode::Honours) if start < total => {
                        let mut head = format!(
                            "HTTP/1.1 206 Partial Content\r\ncontent-range: bytes {start}-{}/{total}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            total - 1,
                            total - start
                        )
                        .into_bytes();
                        head.extend_from_slice(&body[start..]);
                        head
                    }
                    (Some(start), RangeMode::WrongStart) => {
                        let wrong = start / 2;
                        let mut head = format!(
                            "HTTP/1.1 206 Partial Content\r\ncontent-range: bytes {wrong}-{}/{total}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            total - 1,
                            total - wrong
                        )
                        .into_bytes();
                        head.extend_from_slice(&body[wrong..]);
                        head
                    }
                    (Some(_), RangeMode::NotSatisfiable) => format!(
                        "HTTP/1.1 416 Range Not Satisfiable\r\ncontent-range: bytes */{total}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    )
                    .into_bytes(),
                    _ => {
                        let mut head = format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {total}\r\nconnection: close\r\n\r\n"
                        )
                        .into_bytes();
                        head.extend_from_slice(&body);
                        head
                    }
                };
                let _ = stream.write_all(&response).await;
            }
        });
        (url, seen)
    }

    async fn read_all(original: &mut AssetOriginal) -> Vec<u8> {
        let mut body = Vec::new();
        while let Some(chunk) = original.next_chunk().await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        body
    }

    fn bytes_of(length: usize) -> Vec<u8> {
        (0..length).map(|index| (index % 251) as u8).collect()
    }

    #[tokio::test]
    async fn a_read_from_the_middle_asks_for_a_range_and_gets_only_the_rest() {
        let data = bytes_of(20_000);
        let (url, seen) = serve_original(data.clone(), RangeMode::Honours).await;

        let mut original = test_client(url, "key")
            .open_asset_original_from("a", 8_192, OriginalTimeouts::default())
            .await
            .unwrap();

        assert_eq!(original.starts_at(), 8_192);
        assert_eq!(
            original.content_length(),
            Some(20_000),
            "the whole original"
        );
        assert_eq!(read_all(&mut original).await, &data[8_192..]);
        assert_eq!(*seen.lock().unwrap(), [Some("bytes=8192-".to_string())]);
    }

    #[tokio::test]
    async fn a_read_from_the_start_sends_no_range() {
        let data = bytes_of(5_000);
        let (url, seen) = serve_original(data.clone(), RangeMode::Honours).await;

        let mut original = test_client(url, "key")
            .open_asset_original_from("a", 0, OriginalTimeouts::default())
            .await
            .unwrap();

        assert_eq!(original.starts_at(), 0);
        assert_eq!(read_all(&mut original).await, data);
        assert_eq!(*seen.lock().unwrap(), [None]);
    }

    #[tokio::test]
    async fn a_server_that_ignores_the_range_gives_the_whole_original_from_zero() {
        let data = bytes_of(20_000);
        let (url, _seen) = serve_original(data.clone(), RangeMode::Ignores).await;

        let mut original = test_client(url, "key")
            .open_asset_original_from("a", 8_192, OriginalTimeouts::default())
            .await
            .unwrap();

        assert_eq!(original.starts_at(), 0);
        assert_eq!(original.content_length(), Some(20_000));
        assert_eq!(read_all(&mut original).await, data);
    }

    #[tokio::test]
    async fn an_unsatisfiable_range_or_one_that_starts_elsewhere_falls_back_to_the_whole_original()
    {
        for mode in [RangeMode::NotSatisfiable, RangeMode::WrongStart] {
            let data = bytes_of(20_000);
            let (url, seen) = serve_original(data.clone(), mode).await;

            let mut original = test_client(url, "key")
                .open_asset_original_from("a", 8_192, OriginalTimeouts::default())
                .await
                .unwrap();

            assert_eq!(original.starts_at(), 0);
            assert_eq!(read_all(&mut original).await, data);
            assert_eq!(
                *seen.lock().unwrap(),
                [Some("bytes=8192-".to_string()), None],
                "asked once with the range, then once for the whole original"
            );
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
            .open_asset_original_from("a", 0, quick())
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
            .open_asset_original_from("a", 0, quick())
            .await
            .unwrap();

        assert_eq!(original.next_chunk().await.unwrap().unwrap(), "hello");
        let error = original.next_chunk().await.unwrap_err();

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
        assert!(error.message.contains("no data"), "{}", error.message);
        hold.abort();
    }
}
