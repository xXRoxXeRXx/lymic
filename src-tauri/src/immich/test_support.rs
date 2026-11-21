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

/// One request received by [`serve_routes`].
#[derive(Debug, Clone)]
pub(crate) struct RecordedRequest {
    pub method: String,
    /// The request target, path and query string (`/api/search/metadata`).
    pub target: String,
    pub body: String,
}

/// A fake Immich server that answers every request through a handler, however many arrive and
/// in whatever order, and records them. Unlike [`serve_pages`] it routes on the request, so a
/// test can describe a whole Library instead of predicting the request sequence.
pub(crate) struct RoutedServer {
    pub url: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<RecordedRequest>>>,
    task: JoinHandle<()>,
}

impl RoutedServer {
    /// Every request received so far, oldest first.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for RoutedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Serves requests until dropped. The handler returns `(status line, JSON body)`.
pub(crate) async fn serve_routes<H>(handler: H) -> RoutedServer
where
    H: Fn(&RecordedRequest) -> (String, String) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let handler = std::sync::Arc::new(handler);
    let log = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (handler, log) = (handler.clone(), log.clone());
            tokio::spawn(async move {
                let Some(request) = read_request(&mut stream).await else {
                    return;
                };
                let (status, body) = handler(&request);
                log.lock().unwrap().push(request);
                let headers = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(headers.as_bytes()).await;
                let _ = stream.write_all(body.as_bytes()).await;
            });
        }
    });
    RoutedServer {
        url,
        requests,
        task,
    }
}

/// Reads one HTTP request: the head, then as many body bytes as `content-length` announces.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<RecordedRequest> {
    let mut received = Vec::new();
    let mut chunk = [0_u8; 8192];
    let head_end = loop {
        if let Some(position) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        received.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&received[..head_end]).into_owned();
    let content_length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while received.len() < head_end + content_length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        received.extend_from_slice(&chunk[..read]);
    }
    let mut request_line = head.lines().next()?.split_whitespace();
    Some(RecordedRequest {
        method: request_line.next()?.to_string(),
        target: request_line.next()?.to_string(),
        body: String::from_utf8_lossy(&received[head_end..head_end + content_length]).into_owned(),
    })
}
