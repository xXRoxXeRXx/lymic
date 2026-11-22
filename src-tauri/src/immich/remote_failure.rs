//! A typed failure of an Immich request, for callers that must act on *why* it failed (the Virtual
//! Library picks a Cloud Files error from it, ADR-0003) and so cannot parse message text.
//!
//! The kind is decided once, where the HTTP status is known, and travels with the message. The
//! message never carries credentials or a response body.

use super::response::response_status_error;
use reqwest::{RequestBuilder, Response, StatusCode};
use std::time::Duration;

/// Why a request failed, in the terms a reading program can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteFailureKind {
    /// 401 or 403, or no stored credentials: the API key is missing or no longer accepted.
    Unauthorized,
    /// 404: the Remote Asset no longer exists (or is not visible to this key).
    NotFound,
    /// The server could not be reached, did not answer in time, or answered with an error or a
    /// broken body.
    Unavailable,
}

/// A failed request. `message` never carries credentials or a response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteFailure {
    pub kind: RemoteFailureKind,
    pub message: String,
}

impl RemoteFailureKind {
    pub(crate) fn of_status(status: StatusCode) -> Self {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::Unauthorized,
            StatusCode::NOT_FOUND => Self::NotFound,
            _ => Self::Unavailable,
        }
    }
}

impl RemoteFailure {
    pub(crate) fn new(kind: RemoteFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self::new(RemoteFailureKind::Unavailable, message)
    }

    /// The failure of a response that is not a success.
    pub(crate) async fn of_response(failure: &str, response: Response) -> Self {
        let kind = RemoteFailureKind::of_status(response.status());
        let detail = response_status_error(response)
            .await
            .unwrap_or_else(|error| error);
        Self::new(kind, format!("{failure}: {detail}"))
    }
}

/// Sends `request`, giving up after `timeout` (connecting, sending and waiting for the response
/// headers; a streamed body is not covered, the caller bounds that itself). Returns the response
/// only when it succeeded.
pub(crate) async fn send_bounded(
    request: RequestBuilder,
    failure: &str,
    timeout: Duration,
) -> Result<Response, RemoteFailure> {
    let sent = tokio::time::timeout(timeout, request.send()).await;
    let response = match sent {
        Err(_) => {
            return Err(RemoteFailure::unavailable(format!(
                "{failure}: the server did not answer within {} seconds",
                timeout.as_secs().max(1)
            )))
        }
        Ok(Err(error)) => return Err(RemoteFailure::unavailable(format!("{failure}: {error}"))),
        Ok(Ok(response)) => response,
    };
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(RemoteFailure::of_response(failure, response).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::test_support::{serve_once, test_client};
    use reqwest::Method;
    use tokio::net::TcpListener;

    const FAST: Duration = Duration::from_secs(5);

    async fn send(url: String, timeout: Duration) -> Result<Response, RemoteFailure> {
        let client = test_client(url, "key");
        send_bounded(
            client.request(Method::GET, "ping"),
            "Failed to ping",
            timeout,
        )
        .await
    }

    #[tokio::test]
    async fn the_status_of_a_refusal_decides_the_kind_and_no_body_is_forwarded() {
        for (status, kind) in [
            ("401 Unauthorized", RemoteFailureKind::Unauthorized),
            ("403 Forbidden", RemoteFailureKind::Unauthorized),
            ("404 Not Found", RemoteFailureKind::NotFound),
            ("500 Internal Server Error", RemoteFailureKind::Unavailable),
            ("302 Found", RemoteFailureKind::Unavailable),
        ] {
            let (url, _request) = serve_once(status, "secret server detail").await;

            let error = send(url, FAST).await.expect_err(status);

            assert_eq!(error.kind, kind, "{status}");
            assert!(
                error.message.starts_with("Failed to ping"),
                "{}",
                error.message
            );
            assert!(
                !error.message.contains("secret server detail"),
                "{}",
                error.message
            );
        }
    }

    #[tokio::test]
    async fn a_success_comes_back_as_the_response() {
        let (url, _request) = serve_once("200 OK", "pong").await;

        let response = send(url, FAST).await.unwrap();

        assert_eq!(response.text().await.unwrap(), "pong");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let error = send(url, FAST).await.unwrap_err();

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
    }

    #[tokio::test]
    async fn a_server_that_never_answers_is_unavailable_after_the_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        // Accepts and then says nothing, like a blackholed or hung server.
        let hold = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let started = std::time::Instant::now();
        let error = send(url, Duration::from_millis(150)).await.unwrap_err();

        assert_eq!(error.kind, RemoteFailureKind::Unavailable);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            error.message.contains("did not answer"),
            "{}",
            error.message
        );
        hold.abort();
    }
}
