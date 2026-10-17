//! Private, local-only protocol between the Lymic process and Explorer's
//! thumbnail provider. It deliberately transports cached thumbnail bytes,
//! never a placeholder file path or its contents.

const REQUEST_MAGIC: [u8; 4] = *b"LYTH";
const RESPONSE_MAGIC: [u8; 4] = *b"LYTR";
const PROTOCOL_VERSION: u8 = 1;
const REQUEST_HEADER_LEN: usize = 18;
const RESPONSE_HEADER_LEN: usize = 20;
const MAX_IDENTITY_PART_LEN: usize = u16::MAX as usize;
const MAX_IMAGE_LEN: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ThumbnailRequest {
    pub(crate) asset_id: String,
    pub(crate) version: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl ThumbnailRequest {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let asset_id = self.asset_id.as_bytes();
        let version = self.version.as_bytes();
        let mut frame = Vec::with_capacity(REQUEST_HEADER_LEN + asset_id.len() + version.len());
        frame.extend_from_slice(&REQUEST_MAGIC);
        frame.push(PROTOCOL_VERSION);
        frame.push(0);
        frame.extend_from_slice(&(asset_id.len() as u16).to_le_bytes());
        frame.extend_from_slice(&(version.len() as u16).to_le_bytes());
        frame.extend_from_slice(&self.width.to_le_bytes());
        frame.extend_from_slice(&self.height.to_le_bytes());
        frame.extend_from_slice(asset_id);
        frame.extend_from_slice(version);
        frame
    }

    pub(crate) fn decode(frame: &[u8]) -> Result<Self, &'static str> {
        if frame.len() < REQUEST_HEADER_LEN || frame[..4] != REQUEST_MAGIC {
            return Err("invalid thumbnail request header");
        }
        if frame[4] != PROTOCOL_VERSION {
            return Err("unsupported thumbnail request version");
        }
        let asset_id_len = u16::from_le_bytes([frame[6], frame[7]]) as usize;
        let version_len = u16::from_le_bytes([frame[8], frame[9]]) as usize;
        let length = REQUEST_HEADER_LEN
            .checked_add(asset_id_len)
            .and_then(|value| value.checked_add(version_len))
            .ok_or("thumbnail request is too long")?;
        if asset_id_len == 0
            || asset_id_len > MAX_IDENTITY_PART_LEN
            || version_len > MAX_IDENTITY_PART_LEN
            || frame.len() != length
        {
            return Err("invalid thumbnail request identity");
        }
        let asset_id =
            std::str::from_utf8(&frame[REQUEST_HEADER_LEN..REQUEST_HEADER_LEN + asset_id_len])
                .map_err(|_| "thumbnail asset id is not UTF-8")?
                .to_owned();
        let version = std::str::from_utf8(&frame[REQUEST_HEADER_LEN + asset_id_len..])
            .map_err(|_| "thumbnail version is not UTF-8")?
            .to_owned();
        Ok(Self {
            asset_id,
            version,
            width: u32::from_le_bytes([frame[10], frame[11], frame[12], frame[13]]),
            height: u32::from_le_bytes([frame[14], frame[15], frame[16], frame[17]]),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ThumbnailResponse {
    Miss,
    Hit {
        image: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl ThumbnailResponse {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let (status, image, width, height) = match self {
            Self::Miss => (0, &[][..], 0, 0),
            Self::Hit {
                image,
                width,
                height,
            } => (1, image.as_slice(), *width, *height),
        };
        let mut frame = Vec::with_capacity(RESPONSE_HEADER_LEN + image.len());
        frame.extend_from_slice(&RESPONSE_MAGIC);
        frame.push(PROTOCOL_VERSION);
        frame.push(status);
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&width.to_le_bytes());
        frame.extend_from_slice(&height.to_le_bytes());
        frame.extend_from_slice(&(image.len() as u32).to_le_bytes());
        frame.extend_from_slice(image);
        frame
    }

    pub(crate) fn decode(frame: &[u8]) -> Result<Self, &'static str> {
        if frame.len() < RESPONSE_HEADER_LEN || frame[..4] != RESPONSE_MAGIC {
            return Err("invalid thumbnail response header");
        }
        if frame[4] != PROTOCOL_VERSION {
            return Err("unsupported thumbnail response version");
        }
        let status = frame[5];
        let width = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
        let height = u32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]]);
        let image_len = u32::from_le_bytes([frame[16], frame[17], frame[18], frame[19]]) as usize;
        if image_len > MAX_IMAGE_LEN || frame.len() != RESPONSE_HEADER_LEN + image_len {
            return Err("invalid thumbnail response payload");
        }
        match status {
            0 if image_len == 0 => Ok(Self::Miss),
            1 if image_len > 0 => Ok(Self::Hit {
                image: frame[RESPONSE_HEADER_LEN..].to_vec(),
                width,
                height,
            }),
            _ => Err("invalid thumbnail response status"),
        }
    }
}

#[cfg(windows)]
pub(crate) const PIPE_NAME: &str = r"\\.\pipe\lymic-thumbnail-cache-v1";

/// Starts the local cache-only endpoint used by the Explorer COM server.
///
/// This server only reads files already present in `ThumbnailCache`; it never
/// calls Immich and never opens a Virtual Placeholder.
#[cfg(windows)]
pub(crate) fn start_server(cache: std::sync::Arc<crate::thumbnail_cache::ThumbnailCache>) {
    std::thread::Builder::new()
        .name("thumbnail-pipe".into())
        .spawn(move || native::serve(cache))
        .expect("could not start thumbnail pipe server");
}

#[cfg(windows)]
mod native {
    use super::{ThumbnailRequest, ThumbnailResponse, PIPE_NAME, REQUEST_HEADER_LEN};
    use crate::thumbnail_cache::ThumbnailCache;
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX},
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
            PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        },
    };

    const MAX_REQUEST_LEN: usize = REQUEST_HEADER_LEN + 2 * u16::MAX as usize;

    pub(super) fn serve(cache: Arc<ThumbnailCache>) {
        loop {
            let pipe = unsafe {
                CreateNamedPipeW(
                    wide(PIPE_NAME).as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    16 * 1024,
                    16 * 1024,
                    0,
                    std::ptr::null(),
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                return;
            }
            let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) } != 0
                || std::io::Error::last_os_error().raw_os_error()
                    == Some(ERROR_PIPE_CONNECTED as i32);
            if connected {
                let response = read_request(pipe)
                    .and_then(|request| cached_response(&cache, request))
                    .unwrap_or(ThumbnailResponse::Miss);
                let _ = write_all(pipe, &response.encode());
            }
            unsafe { CloseHandle(pipe) };
        }
    }

    fn cached_response(
        cache: &ThumbnailCache,
        request: ThumbnailRequest,
    ) -> Option<ThumbnailResponse> {
        if let Some(path) = cache.get(&request.asset_id, &request.version) {
            if let Ok(image) = std::fs::read(path) {
                if !image.is_empty() {
                    return Some(ThumbnailResponse::Hit {
                        image,
                        width: request.width,
                        height: request.height,
                    });
                }
            }
        }
        if let Ok(Some(credentials)) = crate::auth::get_credentials() {
            if let Ok(client) = crate::immich::ImmichClient::new(
                credentials.server_url,
                credentials.api_key,
                credentials.allow_local_http,
                credentials.cloudflare_client_id,
                credentials.cloudflare_client_secret,
            ) {
                let remote_id = crate::immich::RemoteAssetId::new(&request.asset_id);
                if let Ok(path) = tauri::async_runtime::block_on(cache.get_or_fetch(
                    &client,
                    &remote_id,
                    &request.version,
                )) {
                    if let Ok(image) = std::fs::read(path) {
                        if !image.is_empty() {
                            return Some(ThumbnailResponse::Hit {
                                image,
                                width: request.width,
                                height: request.height,
                            });
                        }
                    }
                }
            }
        }
        None
    }

    fn read_request(pipe: HANDLE) -> Option<ThumbnailRequest> {
        let mut header = [0_u8; REQUEST_HEADER_LEN];
        read_exact(pipe, &mut header)?;
        let asset_len = u16::from_le_bytes([header[6], header[7]]) as usize;
        let version_len = u16::from_le_bytes([header[8], header[9]]) as usize;
        let len = REQUEST_HEADER_LEN
            .checked_add(asset_len)?
            .checked_add(version_len)?;
        if len > MAX_REQUEST_LEN {
            return None;
        }
        let mut frame = header.to_vec();
        frame.resize(len, 0);
        read_exact(pipe, &mut frame[REQUEST_HEADER_LEN..])?;
        ThumbnailRequest::decode(&frame).ok()
    }

    fn read_exact(pipe: HANDLE, buffer: &mut [u8]) -> Option<()> {
        let mut offset = 0;
        while offset < buffer.len() {
            let mut count = 0;
            let size = u32::try_from(buffer.len() - offset).ok()?;
            if unsafe {
                ReadFile(
                    pipe,
                    buffer[offset..].as_mut_ptr().cast(),
                    size,
                    &mut count,
                    std::ptr::null_mut(),
                )
            } == 0
                || count == 0
            {
                return None;
            }
            offset += count as usize;
        }
        Some(())
    }

    fn write_all(pipe: HANDLE, buffer: &[u8]) -> Option<()> {
        let mut offset = 0;
        while offset < buffer.len() {
            let mut count = 0;
            let size = u32::try_from(buffer.len() - offset).ok()?;
            if unsafe {
                WriteFile(
                    pipe,
                    buffer[offset..].as_ptr().cast(),
                    size,
                    &mut count,
                    std::ptr::null_mut(),
                )
            } == 0
                || count == 0
            {
                return None;
            }
            offset += count as usize;
        }
        Some(())
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_preserves_placeholder_identity_and_dimensions() {
        let request = ThumbnailRequest {
            asset_id: "remote-asset".into(),
            version: "2026-10-16T12:00:00Z".into(),
            width: 320,
            height: 180,
        };

        assert_eq!(
            ThumbnailRequest::decode(&request.encode()).unwrap(),
            request
        );
    }

    #[test]
    fn cache_miss_response_has_no_payload() {
        let response = ThumbnailResponse::Miss;

        assert_eq!(
            ThumbnailResponse::decode(&response.encode()).unwrap(),
            response
        );
    }

    #[test]
    fn hit_response_round_trip_preserves_image_bytes_and_dimensions() {
        let response = ThumbnailResponse::Hit {
            image: vec![0x89, b'P', b'N', b'G'],
            width: 320,
            height: 180,
        };

        assert_eq!(
            ThumbnailResponse::decode(&response.encode()).unwrap(),
            response
        );
    }
}
