//! A fake Immich server shared by the endpoint tests.

use super::ImmichClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Serves the given `(status line, body)` responses in order, one connection each,
/// and returns the base URL plus a handle that yields the request texts received.
pub(crate) async fn serve_pages<S, B>(responses: Vec<(S, B)>) -> (String, JoinHandle<Vec<String>>)
where
    S: Into<String> + Send + 'static,
    B: Into<String> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (status, body) = (status.into(), body.into());
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
            let headers = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            // Clients enforcing response-size limits may close the socket as soon as headers arrive.
            if let Err(err) = stream.write_all(body.as_bytes()).await {
                if !matches!(
                    err.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ) {
                    panic!("failed to write response body: {err}");
                }
            }
        }
        requests
    });
    (format!("http://{address}"), server)
}

/// Serves one canned response; the handle yields the request line and headers received.
pub(crate) async fn serve_once(status: &str, body: &str) -> (String, JoinHandle<String>) {
    let (url, server) = serve_pages(vec![(status.to_string(), body.to_string())]).await;
    let request = tokio::spawn(async move { server.await.unwrap().remove(0) });
    (url, request)
}

/// A client for a fake server at `url` (plain HTTP is allowed for it).
pub(crate) fn test_client(url: String, api_key: &str) -> ImmichClient {
    ImmichClient::new(url, api_key.to_string(), true, None, None).unwrap()
}
