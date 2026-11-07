use crate::immich::{AssetResponse, ImmichClient};
use tauri::http::{header, Response, StatusCode, Uri};

const MAX_VIDEO_RANGE_CHUNK_BYTES: u64 = 8 * 1024 * 1024;

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
    let (client, credentials, _) = match ImmichClient::from_stored_credentials() {
        Ok(authenticated) => authenticated,
        Err(error) => return error_response(StatusCode::UNAUTHORIZED, &error),
    };
    let result = match kind {
        "video" => {
            let effective_range = bounded_video_range(range);
            client.get_asset(asset_id, Some(&effective_range)).await
        }
        _ => client.get_asset_preview(asset_id).await,
    };
    match result {
        Ok(response) => asset_response(response),
        Err(error) => error_response(
            StatusCode::BAD_GATEWAY,
            &ImmichClient::redact_error(&error, &credentials),
        ),
    }
}

pub(crate) fn protocol_target<'a>(
    uri: &'a Uri,
    allowed_kinds: &[&str],
) -> Option<(&'a str, &'a str)> {
    let (kind, id) = match uri.authority()?.as_str() {
        "localhost" => {
            let path = uri.path().strip_prefix('/')?;
            path.split_once('/')?
        }
        kind => (kind, uri.path().strip_prefix('/')?),
    };
    (allowed_kinds.contains(&kind) && ImmichClient::validate_path_segment(id, "asset ID").is_ok())
        .then_some((kind, id))
}

fn asset_target(uri: &Uri) -> Option<(&str, &str)> {
    protocol_target(uri, &["asset", "video"])
}

fn bounded_video_range(range: Option<&str>) -> String {
    let max_span = MAX_VIDEO_RANGE_CHUNK_BYTES - 1;
    let Some(raw) = range.and_then(|r| r.trim().strip_prefix("bytes=")) else {
        return format!("bytes=0-{max_span}");
    };
    let Some((start_str, end_str)) = raw.split_once('-') else {
        return format!("bytes=0-{max_span}");
    };
    let Ok(start) = start_str.trim().parse::<u64>() else {
        return format!("bytes=0-{max_span}");
    };
    let capped_end = start.saturating_add(max_span);
    match end_str.trim() {
        "" => format!("bytes={start}-{capped_end}"),
        explicit => match explicit.parse::<u64>() {
            Ok(end) if end >= start => format!("bytes={start}-{}", end.min(capped_end)),
            _ => format!("bytes={start}-{capped_end}"),
        },
    }
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

    #[test]
    fn bounds_open_ended_or_missing_video_ranges_while_preserving_small_ranges() {
        assert_eq!(bounded_video_range(None), "bytes=0-8388607");
        assert_eq!(bounded_video_range(Some("bytes=0-")), "bytes=0-8388607");
        assert_eq!(bounded_video_range(Some("bytes=100-")), "bytes=100-8388707");
        assert_eq!(bounded_video_range(Some("bytes=10-19")), "bytes=10-19");
        assert_eq!(
            bounded_video_range(Some("bytes=0-999999999")),
            "bytes=0-8388607"
        );
    }
}
