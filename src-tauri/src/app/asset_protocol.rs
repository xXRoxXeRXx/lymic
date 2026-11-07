use crate::{
    auth,
    immich::{AssetResponse, ImmichClient},
};
use tauri::http::{header, Response, StatusCode, Uri};

pub(crate) fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.register_asynchronous_uri_scheme_protocol(
        "lymic-asset",
        move |_context, request, responder| {
            let uri = request.uri().clone();
            let range = request
                .headers()
                .get(header::RANGE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            tauri::async_runtime::spawn(async move {
                responder.respond(serve(uri, range.as_deref()).await);
            });
        },
    )
}

async fn serve(uri: Uri, range: Option<&str>) -> Response<Vec<u8>> {
    let Some((kind, asset_id)) = asset_target(&uri) else {
        return error_response(StatusCode::BAD_REQUEST, "Invalid asset URI");
    };
    if kind == "video" && range.is_none() {
        // The browser may issue an initial request without Range; Immich still returns a playable response.
    }
    let credentials = match auth::get_credentials()
        .map_err(|error| error.to_string())
        .and_then(|value| value.ok_or_else(|| "Not authenticated".to_string()))
    {
        Ok(credentials) => credentials,
        Err(error) => return error_response(StatusCode::UNAUTHORIZED, &error),
    };
    match ImmichClient::new(
        credentials.server_url,
        credentials.api_key,
        credentials.allow_local_http,
        credentials.cloudflare_client_id,
        credentials.cloudflare_client_secret,
    )
    .and_then(|client| Ok(client))
    {
        Ok(client) => match client.get_asset(asset_id, range).await {
            Ok(response) => asset_response(response),
            Err(error) => error_response(StatusCode::BAD_GATEWAY, &error),
        },
        Err(error) => error_response(StatusCode::BAD_REQUEST, &error),
    }
}

fn asset_target(uri: &Uri) -> Option<(&str, &str)> {
    let (kind, id) = match uri.authority()?.as_str() {
        kind @ ("asset" | "video") => (kind, uri.path().strip_prefix('/')?),
        "localhost" => {
            let path = uri.path().strip_prefix('/')?;
            let (kind, id) = path.split_once('/')?;
            (kind == "asset" || kind == "video").then_some((kind, id))?
        }
        _ => return None,
    };
    (!id.is_empty()
        && !id.contains('/')
        && !id.contains("..")
        && !id.chars().any(char::is_whitespace))
    .then_some((kind, id))
}

fn asset_response(asset: AssetResponse) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(asset.status).unwrap_or(StatusCode::BAD_GATEWAY));
    for (name, value) in [
        (header::CONTENT_TYPE, asset.content_type),
        (header::CONTENT_RANGE, asset.content_range),
        (header::CONTENT_LENGTH, asset.content_length),
        (header::ACCEPT_RANGES, asset.accept_ranges),
    ] {
        if let Some(value) = value {
            builder = builder.header(name, value);
        }
    }
    builder.body(asset.body).unwrap()
}

fn error_response(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_partial_content_headers_for_video_range_requests() {
        let response = asset_response(AssetResponse {
            status: 206,
            content_type: Some("video/mp4".into()),
            content_range: Some("bytes 10-19/100".into()),
            content_length: Some("10".into()),
            accept_ranges: Some("bytes".into()),
            body: vec![0; 10],
        });
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 10-19/100");
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
    }

    #[test]
    fn resolves_asset_and_video_targets_for_direct_and_localhost_uris() {
        assert_eq!(
            asset_target(&"lymic-asset://asset/photo-1".parse().unwrap()),
            Some(("asset", "photo-1"))
        );
        assert_eq!(
            asset_target(&"lymic-asset://video/video-1".parse().unwrap()),
            Some(("video", "video-1"))
        );
        assert_eq!(
            asset_target(&"lymic-asset://localhost/asset/photo-1".parse().unwrap()),
            Some(("asset", "photo-1"))
        );
        assert_eq!(
            asset_target(&"lymic-asset://localhost/video/video-1".parse().unwrap()),
            Some(("video", "video-1"))
        );
        assert!(asset_target(&"lymic-asset://localhost/other/photo-1".parse().unwrap()).is_none());
    }
}
