use reqwest::Response;

pub(super) const MAX_ERROR_RESPONSE_BYTES: usize = 8 * 1024;
pub(super) const MAX_SUCCESS_RESPONSE_BYTES: usize = 1024 * 1024;

pub(super) async fn response_status_error(response: Response) -> String {
    let status = response.status();
    format!("{}{}", status, response_error_detail(response).await)
}

pub(super) async fn response_error_detail(response: Response) -> String {
    read_error_body_capped(response, MAX_ERROR_RESPONSE_BYTES).await
}

pub(super) async fn read_error_body_capped(mut response: Response, max_bytes: usize) -> String {
    let mut body = Vec::with_capacity(max_bytes);
    let mut truncated = false;
    while let Ok(Some(chunk)) = response.chunk().await {
        let remaining = max_bytes.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match (text.is_empty(), truncated) {
        (true, false) => String::new(),
        (true, true) => ": [truncated]".to_string(),
        (false, false) => format!(": {}", text),
        (false, true) => format!(": {}... [truncated]", text),
    }
}

pub(super) async fn read_response_body_limited(
    mut response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!(
            "response body exceeds {} KiB limit",
            max_bytes / 1024
        ));
    }
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(max_bytes as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(format!(
                "response body exceeds {} KiB limit",
                max_bytes / 1024
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::{read_error_body_capped, read_response_body_limited, MAX_ERROR_RESPONSE_BYTES};
    use reqwest::{Client, Response};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn test_response(headers: &str, body: Vec<u8>) -> Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let headers = headers.to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });
        Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn response_body_limit_accepts_exact_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
            b"test".to_vec(),
        )
        .await;
        assert_eq!(
            read_response_body_limited(response, 4).await.unwrap(),
            b"test"
        );
    }

    #[tokio::test]
    async fn response_body_limit_rejects_oversized_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
            b"tests".to_vec(),
        )
        .await;
        assert!(read_response_body_limited(response, 4)
            .await
            .unwrap_err()
            .contains("exceeds"));
    }

    #[tokio::test]
    async fn response_body_limit_rejects_oversized_body_without_content_length() {
        let response = test_response(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            b"tests".to_vec(),
        )
        .await;
        assert!(read_response_body_limited(response, 4)
            .await
            .unwrap_err()
            .contains("exceeds"));
    }

    #[tokio::test]
    async fn error_body_is_capped_and_marked_as_truncated() {
        let body = vec![b'x'; MAX_ERROR_RESPONSE_BYTES + 1];
        let response = test_response(&format!("HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()), body).await;
        let detail = read_error_body_capped(response, MAX_ERROR_RESPONSE_BYTES).await;
        assert!(detail.ends_with("... [truncated]"));
        assert_eq!(
            detail.len(),
            MAX_ERROR_RESPONSE_BYTES + ": ".len() + "... [truncated]".len()
        );
    }
}
