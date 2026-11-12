use super::bulk_check::{
    bulk_check_response_error, parse_bulk_check_response, BulkCheckDuplicate, BulkCheckError,
};
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use super::upload::{parse_upload_response, upload_form, SidecarDirectoryCache};
use crate::sync::progress::TransferStatsTracker;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Response,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Component, Path};
use tokio::io::AsyncWriteExt;
use url::{Host, Url};

const UPLOAD_TIMEOUT_BASE_SECONDS: u64 = 5 * 60;
const UPLOAD_TIMEOUT_SECONDS_PER_GIB: u64 = 2 * 60 * 60;
const GIB_BYTES: u128 = 1024 * 1024 * 1024;
const MAX_THUMBNAIL_BYTES: usize = 20 * 1024 * 1024;
const MAX_ASSET_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const LOCAL_HTTP_NOT_ALLOWED_ERROR: &str = "LOCAL_HTTP_NOT_ALLOWED";

type CachedAuthenticatedClient = (
    crate::auth::AuthConfig,
    std::sync::Arc<ImmichClient>,
    String,
);

fn authenticated_client_cache() -> &'static std::sync::Mutex<Option<CachedAuthenticatedClient>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<CachedAuthenticatedClient>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn upload_timeout(total_file_bytes: u64) -> std::time::Duration {
    let proportional_seconds = (u128::from(total_file_bytes)
        .saturating_mul(u128::from(UPLOAD_TIMEOUT_SECONDS_PER_GIB))
        .saturating_add(GIB_BYTES - 1))
        / GIB_BYTES;
    let total_seconds =
        u128::from(UPLOAD_TIMEOUT_BASE_SECONDS).saturating_add(proportional_seconds);
    std::time::Duration::from_secs(u64::try_from(total_seconds).unwrap_or(u64::MAX))
}

pub(crate) struct ImmichClient {
    pub(super) client: Client,
    pub(super) server_url: Url,
    pub(super) api_key: String,
}

pub(crate) struct AssetResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_range: Option<String>,
    pub content_length: Option<String>,
    pub accept_ranges: Option<String>,
    pub body: Vec<u8>,
}

pub(crate) struct ArchiveDownload {
    response: Response,
    suggested_file_name: String,
}

impl ArchiveDownload {
    pub(crate) fn suggested_file_name(&self) -> &str {
        &self.suggested_file_name
    }

    pub(crate) async fn save_to(self, destination_path: &Path) -> Result<(), String> {
        ImmichClient::validate_download_destination(destination_path)?;
        stream_response_to_destination(
            self.response,
            destination_path,
            "Failed to download archive",
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssetDetails {
    pub asset_type: Option<String>,
    pub original_file_name: Option<String>,
    pub is_favorite: bool,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub f_number: Option<f64>,
    pub exposure_time: Option<String>,
    pub iso: Option<u32>,
    pub focal_length: Option<f64>,
    pub lens_model: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_size: Option<u64>,
}

#[derive(Deserialize)]
struct AssetDetailsResponse {
    #[serde(rename = "type", default)]
    asset_type: Option<String>,
    #[serde(rename = "originalFileName", default)]
    original_file_name: Option<String>,
    #[serde(rename = "isFavorite", default)]
    is_favorite: bool,
    #[serde(rename = "exifInfo", default)]
    exif: Option<AssetDetailsExif>,
}

#[derive(Default, Deserialize)]
struct AssetDetailsExif {
    #[serde(default)]
    make: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(rename = "fNumber", default)]
    f_number: Option<f64>,
    #[serde(rename = "exposureTime", default)]
    exposure_time: Option<String>,
    #[serde(default)]
    iso: Option<u32>,
    #[serde(rename = "focalLength", default)]
    focal_length: Option<f64>,
    #[serde(rename = "lensModel", default)]
    lens_model: Option<String>,
    #[serde(rename = "exifImageWidth", default)]
    image_width: Option<u32>,
    #[serde(rename = "exifImageHeight", default)]
    image_height: Option<u32>,
    #[serde(rename = "fileSizeInByte", default)]
    file_size: Option<u64>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| (!value.trim().is_empty()).then_some(value))
}

fn original_file_name(value: Option<String>) -> Option<String> {
    non_empty(value).and_then(|value| {
        let file_name = value.rsplit(['/', '\\']).next().unwrap_or_default();
        (!file_name.is_empty()
            && file_name != "."
            && file_name != ".."
            && !file_name.contains(':')
            && !file_name.chars().any(char::is_control))
        .then(|| file_name.to_string())
    })
}

fn content_disposition_file_name(headers: &HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(';').find_map(|parameter| {
                let (name, value) = parameter.trim().split_once('=')?;
                name.eq_ignore_ascii_case("filename")
                    .then(|| value.trim().trim_matches('"').to_string())
            })
        })
        .and_then(|value| original_file_name(Some(value)))
}

async fn stream_response_to_destination(
    mut response: Response,
    destination_path: &Path,
    error_prefix: &str,
) -> Result<(), String> {
    let temporary_path =
        destination_path.with_file_name(format!(".lymic-download-{}.tmp", uuid::Uuid::new_v4()));
    let result = async {
        let mut destination = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await
            .map_err(|error| format!("Failed to create download destination: {error}"))?;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("{error_prefix}: {error}"))?
        {
            destination
                .write_all(&chunk)
                .await
                .map_err(|error| format!("Failed to write download destination: {error}"))?;
        }
        destination
            .flush()
            .await
            .map_err(|error| format!("Failed to write download destination: {error}"))?;
        drop(destination);
        tokio::fs::rename(&temporary_path, destination_path)
            .await
            .map_err(|error| format!("Failed to finalize download: {error}"))
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary_path).await;
    }
    result
}

impl From<AssetDetailsResponse> for AssetDetails {
    fn from(response: AssetDetailsResponse) -> Self {
        let exif = response.exif.unwrap_or_default();
        Self {
            asset_type: response.asset_type,
            original_file_name: original_file_name(response.original_file_name),
            is_favorite: response.is_favorite,
            camera_make: non_empty(exif.make),
            camera_model: non_empty(exif.model),
            city: non_empty(exif.city),
            state: non_empty(exif.state),
            country: non_empty(exif.country),
            f_number: exif.f_number,
            exposure_time: non_empty(exif.exposure_time),
            iso: exif.iso,
            focal_length: exif.focal_length,
            lens_model: non_empty(exif.lens_model),
            image_width: exif.image_width,
            image_height: exif.image_height,
            file_size: exif.file_size,
        }
    }
}

#[derive(Deserialize)]
struct CurrentUserResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
    #[serde(rename = "profileImagePath", default)]
    profile_image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentUserInfo {
    pub id: String,
    pub display_name: String,
    pub has_profile_image: bool,
}

impl ImmichClient {
    pub(crate) fn new(
        server_url: String,
        api_key: String,
        allow_local_http: bool,
        cloudflare_client_id: Option<String>,
        cloudflare_client_secret: Option<String>,
    ) -> Result<Self, String> {
        let mut base_url =
            Url::parse(server_url.trim()).map_err(|_| "Invalid server URL".to_string())?;
        match base_url.scheme() {
            "https" => {}
            "http" if allow_local_http && is_loopback_host(base_url.host()) => {}
            "http" => {
                return Err(
                    format!(
                        "{LOCAL_HTTP_NOT_ALLOWED_ERROR}: HTTP is allowed only for localhost and loopback addresses. All other servers require HTTPS."
                    ),
                )
            }
            _ => return Err("Only HTTPS server URLs are supported".to_string()),
        }
        if base_url.host().is_none() {
            return Err("Server URL must include a host".to_string());
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err("Server URL must not contain credentials, query, or fragment".to_string());
        }
        let path = base_url.path().trim_end_matches('/');
        let api_path = if path.ends_with("/api") {
            format!("{}/", path)
        } else if path.is_empty() {
            "/api/".to_string()
        } else {
            format!("{}/api/", path)
        };
        base_url.set_path(&api_path);
        let default_headers = match (cloudflare_client_id, cloudflare_client_secret) {
            (None, None) => HeaderMap::new(),
            (Some(client_id), Some(client_secret)) => {
                let mut headers = HeaderMap::new();
                let client_id = HeaderValue::from_str(&client_id)
                    .map_err(|_| "Invalid Cloudflare Access client ID".to_string())?;
                let mut client_secret = HeaderValue::from_str(&client_secret)
                    .map_err(|_| "Invalid Cloudflare Access client secret".to_string())?;
                client_secret.set_sensitive(true);
                headers.insert(HeaderName::from_static("cf-access-client-id"), client_id);
                headers.insert(
                    HeaderName::from_static("cf-access-client-secret"),
                    client_secret,
                );
                headers
            }
            _ => {
                return Err(
                    "Cloudflare Access client ID and client secret must both be provided"
                        .to_string(),
                )
            }
        };
        Ok(Self {
            // Reject redirects so an HTTPS endpoint cannot downgrade an API-key request to HTTP.
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .default_headers(default_headers)
                .build()
                .map_err(|_| "Failed to initialize HTTP client".to_string())?,
            server_url: base_url,
            api_key,
        })
    }

    pub(super) fn endpoint_url(&self, path: &str) -> Url {
        self.server_url
            .join(path.trim_start_matches('/'))
            .expect("validated base URL must support relative endpoint paths")
    }

    pub(crate) fn from_stored_credentials(
    ) -> Result<(std::sync::Arc<Self>, crate::auth::AuthConfig, String), String> {
        let credentials =
            crate::auth::get_credentials()?.ok_or_else(|| "Not authenticated".to_string())?;
        if let Ok(cache) = authenticated_client_cache().lock() {
            if let Some((ref cached_creds, ref client, ref account_id)) = *cache {
                if cached_creds == &credentials {
                    return Ok((client.clone(), credentials, account_id.clone()));
                }
            }
        }
        let account_id = crate::audit::actor_id(&credentials.server_url, &credentials.api_key);
        let client = std::sync::Arc::new(
            Self::new(
                credentials.server_url.clone(),
                credentials.api_key.clone(),
                credentials.allow_local_http,
                credentials.cloudflare_client_id.clone(),
                credentials.cloudflare_client_secret.clone(),
            )
            .map_err(|error| Self::redact_error(&error, &credentials))?,
        );
        if let Ok(mut cache) = authenticated_client_cache().lock() {
            *cache = Some((credentials.clone(), client.clone(), account_id.clone()));
        }
        Ok((client, credentials, account_id))
    }

    pub(crate) fn redact_error(error: &str, credentials: &crate::auth::AuthConfig) -> String {
        crate::audit::audit_safe_error(
            error,
            &[
                Some(&credentials.server_url),
                Some(&credentials.api_key),
                credentials.cloudflare_client_id.as_deref(),
                credentials.cloudflare_client_secret.as_deref(),
            ],
        )
    }

    pub(crate) fn validate_path_segment(value: &str, name: &str) -> Result<(), String> {
        if value.trim().is_empty()
            || value.contains("..")
            || value.chars().any(|character| {
                matches!(character, '/' | '\\' | '?' | '#')
                    || character.is_control()
                    || character.is_whitespace()
            })
        {
            return Err(format!("Invalid {name} for API path"));
        }

        Ok(())
    }

    pub(crate) async fn validate_connection(&self) -> Result<(), String> {
        self.current_user_info().await.map(|_| ())
    }

    pub(crate) fn uses_http(&self) -> bool {
        self.server_url.scheme() == "http"
    }

    pub(crate) async fn current_user_info(&self) -> Result<CurrentUserInfo, String> {
        let response = self
            .client
            .get(self.endpoint_url("users/me"))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Connection failed: {}", error))?;
        if !response.status().is_success() {
            return Err(format!(
                "Server returned error: {}",
                response_status_error(response).await?
            ));
        }
        let user: CurrentUserResponse = response
            .json()
            .await
            .map_err(|_| "Invalid current-user response".to_string())?;
        let display_name = if user.name.trim().is_empty() {
            user.email.trim()
        } else {
            user.name.trim()
        };
        if display_name.is_empty() {
            return Err("Current user response did not include a name".to_string());
        }
        let has_profile_image = user
            .profile_image_path
            .as_deref()
            .map(|path| !path.trim().is_empty())
            .unwrap_or(false);
        Ok(CurrentUserInfo {
            id: user.id,
            display_name: display_name.to_string(),
            has_profile_image,
        })
    }

    pub(crate) async fn get_profile_image(
        &self,
        user_id: &str,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        const MAX_PROFILE_IMAGE_BYTES: usize = 10 * 1024 * 1024;
        Self::validate_path_segment(user_id, "user ID")?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("users/{user_id}/profile-image")))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Connection failed: {}", error))?;
        if !response.status().is_success() {
            return Err(format!(
                "Server returned error: {}",
                response_status_error(response).await?
            ));
        }
        if let Some(len) = response.content_length() {
            if len as usize > MAX_PROFILE_IMAGE_BYTES {
                return Err("Profile image exceeds maximum allowed size".to_string());
            }
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .filter(|ct| ct.starts_with("image/"))
            .map(|s| s.to_string());
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read profile image: {}", e))?
            .to_vec();
        if bytes.len() > MAX_PROFILE_IMAGE_BYTES {
            return Err("Profile image exceeds maximum allowed size".to_string());
        }
        Ok((bytes, content_type))
    }

    pub(crate) async fn get_thumbnail(&self, asset_id: &str) -> Result<Vec<u8>, String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("assets/{asset_id}/thumbnail")))
            .query(&[("format", "WEBP")])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch thumbnail: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch thumbnail: {}",
                response_status_error(response).await?
            ));
        }
        read_response_body_limited(response, MAX_THUMBNAIL_BYTES)
            .await
            .map_err(|error| format!("Failed to read thumbnail: {error}"))
    }

    pub(crate) async fn get_person_thumbnail(&self, person_id: &str) -> Result<Vec<u8>, String> {
        Self::validate_path_segment(person_id, "person ID")?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("people/{person_id}/thumbnail")))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch person thumbnail: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch person thumbnail: {}",
                response_status_error(response).await?
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Failed to read person thumbnail: {error}"))?;
        if bytes.len() > MAX_THUMBNAIL_BYTES {
            return Err("Person thumbnail exceeds maximum allowed size".to_string());
        }
        Ok(bytes.to_vec())
    }

    pub(crate) async fn get_asset_preview(&self, asset_id: &str) -> Result<AssetResponse, String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("assets/{asset_id}/thumbnail")))
            .query(&[("size", "preview")])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch asset preview: {error}"))?;
        Self::read_asset_response(response, MAX_THUMBNAIL_BYTES).await
    }

    pub(crate) async fn get_asset_details(&self, asset_id: &str) -> Result<AssetDetails, String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("assets/{asset_id}")))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch asset details: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch asset details: {}",
                response_status_error(response).await?
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES)
            .await
            .map_err(|error| format!("Failed to read asset details: {error}"))?;
        serde_json::from_slice::<AssetDetailsResponse>(&body)
            .map(AssetDetails::from)
            .map_err(|error| format!("Invalid asset details response: {error}"))
    }

    pub(crate) async fn set_asset_favorite(
        &self,
        asset_id: &str,
        is_favorite: bool,
    ) -> Result<(), String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        let response = self
            .client
            .put(self.endpoint_url(&format!("assets/{asset_id}")))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "isFavorite": is_favorite }))
            .send()
            .await
            .map_err(|error| format!("Failed to update asset favorite: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to update asset favorite: {}",
                response_status_error(response).await?
            ));
        }
        Ok(())
    }

    pub(crate) async fn get_asset(
        &self,
        asset_id: &str,
        range: Option<&str>,
    ) -> Result<AssetResponse, String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        let mut request = self
            .client
            .get(self.endpoint_url(&format!("assets/{asset_id}/original")))
            .header("x-api-key", &self.api_key);
        if let Some(range) = range {
            request = request.header(reqwest::header::RANGE, range);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Failed to fetch asset: {error}"))?;
        Self::read_asset_response(response, MAX_ASSET_RESPONSE_BYTES).await
    }

    pub(crate) async fn download_asset(
        &self,
        asset_id: &str,
        destination_path: &Path,
    ) -> Result<(), String> {
        Self::validate_path_segment(asset_id, "asset ID")?;
        Self::validate_download_destination(destination_path)?;
        let response = self
            .client
            .get(self.endpoint_url(&format!("assets/{asset_id}/original")))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to download asset: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to download asset: {}",
                response_status_error(response).await?
            ));
        }

        stream_response_to_destination(response, destination_path, "Failed to download asset").await
    }

    pub(crate) async fn prepare_archive_download(
        &self,
        asset_ids: &[String],
    ) -> Result<ArchiveDownload, String> {
        if asset_ids.is_empty() {
            return Err("At least one asset is required for an archive download".to_string());
        }
        for asset_id in asset_ids {
            Self::validate_path_segment(asset_id, "asset ID")?;
        }
        let response = self
            .client
            .post(self.endpoint_url("download/archive"))
            .header("x-api-key", &self.api_key)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .json(&json!({ "assetIds": asset_ids }))
            .send()
            .await
            .map_err(|error| format!("Failed to download archive: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to download archive: {}",
                response_status_error(response).await?
            ));
        }
        let suggested_file_name = content_disposition_file_name(response.headers())
            .unwrap_or_else(|| "immich-download.zip".to_string());
        Ok(ArchiveDownload {
            response,
            suggested_file_name,
        })
    }

    fn validate_download_destination(destination_path: &Path) -> Result<(), String> {
        if !destination_path.is_absolute()
            || destination_path.file_name().is_none()
            || destination_path.is_dir()
            || !destination_path.parent().is_some_and(Path::is_dir)
            || destination_path
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err("Invalid download destination path".to_string());
        }
        Ok(())
    }

    async fn read_asset_response(
        response: reqwest::Response,
        max_bytes: usize,
    ) -> Result<AssetResponse, String> {
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Failed to fetch asset: {}",
                response_status_error(response).await?
            ));
        }
        let headers = response.headers().clone();
        let body = read_response_body_limited(response, max_bytes)
            .await
            .map_err(|error| format!("Failed to read asset: {error}"))?;
        let header = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        Ok(AssetResponse {
            status: status.as_u16(),
            content_type: header(reqwest::header::CONTENT_TYPE),
            content_range: header(reqwest::header::CONTENT_RANGE),
            content_length: header(reqwest::header::CONTENT_LENGTH),
            accept_ranges: header(reqwest::header::ACCEPT_RANGES),
            body,
        })
    }

    pub(crate) async fn check_assets_exist(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<BulkCheckDuplicate>, BulkCheckError> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let assets: Vec<serde_json::Value> = hashes
            .iter()
            .map(|hash| json!({ "id": hash, "checksum": hash }))
            .collect();
        let response = self
            .client
            .post(self.endpoint_url("assets/bulk-upload-check"))
            .header("x-api-key", &self.api_key)
            .json(&json!({ "assets": assets }))
            .send()
            .await
            .map_err(|error| {
                BulkCheckError::Retryable(format!("Check request failed: {}", error))
            })?;
        if !response.status().is_success() {
            return Err(bulk_check_response_error(response).await);
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES)
            .await
            .map_err(|error| {
                BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", error))
            })?;
        let data = serde_json::from_slice(&body).map_err(|error| {
            BulkCheckError::Retryable(format!("Invalid bulk-check response: {}", error))
        })?;
        parse_bulk_check_response(&data, &hashes).map_err(BulkCheckError::Retryable)
    }

    pub(crate) async fn upload_asset_with_live_photo(
        &self,
        path: &str,
        precomputed_hash: &str,
        live_photo_path: Option<&str>,
        transfer_tracker: Option<TransferStatsTracker>,
        sidecar_cache: SidecarDirectoryCache,
    ) -> Result<String, String> {
        let payload = upload_form(path, live_photo_path, transfer_tracker, sidecar_cache).await?;
        self.post_upload_form(
            payload.form,
            precomputed_hash,
            upload_timeout(payload.total_file_bytes),
        )
        .await
    }

    async fn post_upload_form(
        &self,
        form: reqwest::multipart::Form,
        precomputed_hash: &str,
        timeout: std::time::Duration,
    ) -> Result<String, String> {
        let response: Response = self
            .client
            .post(self.endpoint_url("assets"))
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", precomputed_hash)
            .timeout(timeout)
            .multipart(form)
            .send()
            .await
            .map_err(|error| format!("Network error during upload: {}", error))?;
        if response.status().is_success() {
            parse_upload_response(
                &read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?,
            )
        } else {
            Err(format!(
                "Upload failed: {}",
                response_status_error(response).await?
            ))
        }
    }
}

fn is_loopback_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        original_file_name, upload_timeout, BulkCheckDuplicate, ImmichClient,
        SidecarDirectoryCache, GIB_BYTES, LOCAL_HTTP_NOT_ALLOWED_ERROR,
        UPLOAD_TIMEOUT_BASE_SECONDS,
    };
    use reqwest::Client;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request");
            request.extend_from_slice(&chunk[..read]);
            if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length: ")
                    .or_else(|| line.strip_prefix("Content-Length: "))
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request body");
            request.extend_from_slice(&chunk[..read]);
        }
        request
    }

    async fn read_http_headers(stream: &mut tokio::net::TcpStream) {
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock server received an incomplete request");
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                return;
            }
        }
    }

    fn temporary_file(contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lymic-immich-client-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            Uuid::new_v4(),
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn cloudflare_test_client() -> Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("cf-access-client-id", "client-id".parse().unwrap());
        headers.insert("cf-access-client-secret", "client-secret".parse().unwrap());
        Client::builder().default_headers(headers).build().unwrap()
    }

    #[test]
    fn reduces_remote_original_file_names_to_safe_basenames() {
        assert_eq!(
            original_file_name(Some("../../Pictures/IMG_0001.CR3".to_string())).as_deref(),
            Some("IMG_0001.CR3")
        );
        assert_eq!(
            original_file_name(Some(r"C:\Pictures\IMG_0001.CR3".to_string())).as_deref(),
            Some("IMG_0001.CR3")
        );
        assert_eq!(original_file_name(Some("..".to_string())), None);
    }

    #[tokio::test]
    async fn forwards_video_range_requests_and_preserves_partial_response_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("GET /api/assets/video-1/original HTTP/1.1"));
            assert!(
                request.contains("range: bytes=10-19") || request.contains("Range: bytes=10-19")
            );
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\ncontent-type: video/mp4\r\ncontent-range: bytes 10-19/100\r\ncontent-length: 10\r\naccept-ranges: bytes\r\n\r\n0123456789").await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let response = client
            .get_asset("video-1", Some("bytes=10-19"))
            .await
            .unwrap();

        assert_eq!(response.status, 206);
        assert_eq!(response.content_range.as_deref(), Some("bytes 10-19/100"));
        assert_eq!(response.accept_ranges.as_deref(), Some("bytes"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn streams_an_original_asset_to_a_valid_destination_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("GET /api/assets/photo-1/original HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 12\r\nconnection: close\r\n\r\nfirst-",
                )
                .await
                .unwrap();
            tokio::task::yield_now().await;
            stream.write_all(b"second").await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let destination = temporary_file(b"stale");

        client
            .download_asset("photo-1", &destination)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(&destination).await.unwrap(),
            b"first-second"
        );
        std::fs::remove_file(destination).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn streams_an_immich_archive_to_a_valid_destination_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("POST /api/download/archive HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            assert!(request.contains("{\"assetIds\":[\"photo-1\",\"video-1\"]}"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-disposition: attachment; filename=../../Pictures/immich-export.zip\r\ncontent-length: 12\r\nconnection: close\r\n\r\nfirst-second",
                )
                .await
                .unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let destination = temporary_file(b"stale");

        let archive = client
            .prepare_archive_download(&["photo-1".to_string(), "video-1".to_string()])
            .await
            .unwrap();
        assert_eq!(archive.suggested_file_name(), "immich-export.zip");
        archive.save_to(&destination).await.unwrap();

        assert_eq!(
            tokio::fs::read(&destination).await.unwrap(),
            b"first-second"
        );
        std::fs::remove_file(destination).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn preserves_an_existing_destination_when_an_asset_stream_is_truncated() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("GET /api/assets/photo-1/original HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 12\r\nconnection: close\r\n\r\npartial",
                )
                .await
                .unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let destination = temporary_file(b"complete-original");

        let error = client
            .download_asset("photo-1", &destination)
            .await
            .unwrap_err();

        assert!(error.starts_with("Failed to download asset:"));
        assert_eq!(
            tokio::fs::read(&destination).await.unwrap(),
            b"complete-original"
        );
        std::fs::remove_file(destination).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_download_destination_paths_before_requesting_an_asset() {
        let client = ImmichClient::new(
            "http://127.0.0.1:9".to_string(),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        for destination in [
            std::path::Path::new("download.jpg"),
            std::env::temp_dir().as_path(),
            std::env::temp_dir()
                .join("..")
                .join("download.jpg")
                .as_path(),
        ] {
            assert_eq!(
                client
                    .download_asset("photo-1", destination)
                    .await
                    .unwrap_err(),
                "Invalid download destination path"
            );
        }
    }

    #[tokio::test]
    async fn fetches_lightbox_photo_preview_from_immich_preview_thumbnail_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("GET /api/assets/photo-1/thumbnail?size=preview HTTP/1.1"));
            assert!(request.contains("x-api-key: test-key"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: image/jpeg\r\ncontent-length: 4\r\nconnection: close\r\n\r\njpeg",
                )
                .await
                .unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            None,
            None,
        )
        .unwrap();

        let response = client.get_asset_preview("photo-1").await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type.as_deref(), Some("image/jpeg"));
        assert_eq!(response.body, b"jpeg");
        server.await.unwrap();
    }

    #[test]
    fn rejects_insecure_or_sensitive_server_urls() {
        for server_url in [
            "http://immich.example",
            "http://localhost",
            "https://user:password@immich.example",
            "https://immich.example/?token=secret",
            "https://immich.example/#fragment",
        ] {
            assert!(ImmichClient::new(
                server_url.to_string(),
                "key".to_string(),
                false,
                None,
                None
            )
            .is_err());
        }
    }

    #[test]
    fn normalizes_secure_server_urls() {
        let cases = [
            (
                "https://immich.example",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/api",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/api/",
                "https://immich.example/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example/custom/path",
                "https://immich.example/custom/path/api/assets/bulk-upload-check",
            ),
            (
                "https://immich.example:8443",
                "https://immich.example:8443/api/assets/bulk-upload-check",
            ),
        ];

        for (server_url, expected_endpoint) in cases {
            let client =
                ImmichClient::new(server_url.to_string(), "key".to_string(), false, None, None)
                    .unwrap();
            assert_eq!(
                client.endpoint_url("/assets/bulk-upload-check").as_str(),
                expected_endpoint
            );
        }
    }

    #[test]
    fn rejects_incomplete_or_invalid_cloudflare_access_headers_locally() {
        assert!(ImmichClient::new(
            "https://immich.example".to_string(),
            "key".to_string(),
            false,
            Some("client-id".to_string()),
            None,
        )
        .is_err());
        assert!(ImmichClient::new(
            "https://immich.example".to_string(),
            "key".to_string(),
            false,
            Some("bad\nclient-id".to_string()),
            Some("client-secret".to_string()),
        )
        .is_err());
    }

    #[test]
    fn accepts_opted_in_http_only_for_loopback_hosts() {
        for server_url in [
            "http://localhost",
            "http://127.0.0.1",
            "http://127.255.255.255",
            "http://[::1]",
        ] {
            assert!(
                ImmichClient::new(server_url.to_string(), "key".to_string(), true, None, None)
                    .is_ok()
            );
        }

        for server_url in [
            "http://10.0.0.1",
            "http://192.168.1.1",
            "http://172.16.0.1",
            "http://immich.local",
            "http://immich.example",
            "ftp://localhost",
            "http://localhost/?token=secret",
            "http://localhost/#fragment",
            "http://user:password@localhost",
        ] {
            assert!(
                ImmichClient::new(server_url.to_string(), "key".to_string(), true, None, None)
                    .is_err()
            );
        }
    }

    #[test]
    fn local_http_rejection_uses_a_stable_error_code() {
        let error = ImmichClient::new(
            "http://immich.example".to_string(),
            "key".to_string(),
            true,
            None,
            None,
        )
        .err()
        .unwrap();

        assert!(error.starts_with(LOCAL_HTTP_NOT_ALLOWED_ERROR));
    }

    #[test]
    fn upload_timeout_scales_with_file_payload_size() {
        assert_eq!(
            upload_timeout(0),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS)
        );
        assert_eq!(
            upload_timeout(1),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 1)
        );
        assert_eq!(
            upload_timeout(GIB_BYTES as u64),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 2 * 60 * 60)
        );
        assert_eq!(
            upload_timeout(GIB_BYTES as u64 + 1),
            std::time::Duration::from_secs(UPLOAD_TIMEOUT_BASE_SECONDS + 2 * 60 * 60 + 1)
        );
        assert!(upload_timeout(u64::MAX) > upload_timeout(GIB_BYTES as u64));
    }

    #[tokio::test]
    async fn mock_immich_handles_bulk_check_and_upload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            for response in [
                r#"{"results":[{"id":"already-there","action":"reject","reason":"duplicate"},{"id":"new-file","action":"accept"}]}"#,
                r#"{"id":"uploaded-asset"}"#,
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                requests_tx
                    .send(read_http_request(&mut stream).await)
                    .await
                    .unwrap();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
            }
        });
        let client = ImmichClient {
            client: cloudflare_test_client(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        assert_eq!(
            client
                .check_assets_exist(vec!["already-there".to_string(), "new-file".to_string()])
                .await
                .unwrap(),
            vec![BulkCheckDuplicate {
                hash: "already-there".to_string(),
                asset_id: None,
            }]
        );
        let asset_path = temporary_file(b"image data");
        assert_eq!(
            client
                .upload_asset_with_live_photo(
                    asset_path.to_str().unwrap(),
                    "new-file",
                    None,
                    None,
                    SidecarDirectoryCache::new(),
                )
                .await
                .unwrap(),
            "uploaded-asset"
        );
        std::fs::remove_file(asset_path).unwrap();
        let bulk_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(bulk_request.starts_with("POST /api/assets/bulk-upload-check HTTP/1.1"));
        assert!(bulk_request.contains("x-api-key: test-key"));
        assert!(bulk_request.contains("cf-access-client-id: client-id"));
        assert!(bulk_request.contains("cf-access-client-secret: client-secret"));
        assert!(bulk_request.contains(r#""id":"already-there""#));
        assert!(bulk_request.contains(r#""checksum":"new-file""#));
        let upload_request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(upload_request.starts_with("POST /api/assets HTTP/1.1"));
        assert!(upload_request.contains("x-immich-checksum: new-file"));
        assert!(upload_request.contains("cf-access-client-id: client-id"));
        assert!(upload_request.contains("cf-access-client-secret: client-secret"));
        assert!(upload_request.contains("name=\"assetData\""));
        assert!(!upload_request.contains("name=\"livePhotoData\""));
    }

    #[tokio::test]
    async fn pair_upload_streams_image_and_live_photo_parts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"id\":\"paired\"}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let image = temporary_file(b"image data");
        let video = temporary_file(b"video data");
        let sidecar = image.with_extension("xmp");
        std::fs::write(&sidecar, b"sidecar data").unwrap();
        assert_eq!(
            client
                .upload_asset_with_live_photo(
                    image.to_str().unwrap(),
                    "image-hash",
                    Some(video.to_str().unwrap()),
                    None,
                    SidecarDirectoryCache::new(),
                )
                .await
                .unwrap(),
            "paired"
        );
        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.contains("x-immich-checksum: image-hash"));
        assert!(request.contains("name=\"assetData\""));
        assert!(request.contains("name=\"livePhotoData\""));
        assert!(request.contains("name=\"sidecarData\""));
        assert!(request.contains(image.file_name().unwrap().to_str().unwrap()));
        assert!(request.contains(video.file_name().unwrap().to_str().unwrap()));
        assert!(request.contains(sidecar.file_name().unwrap().to_str().unwrap()));
        std::fs::remove_file(image).unwrap();
        std::fs::remove_file(video).unwrap();
        std::fs::remove_file(sidecar).unwrap();
    }

    #[tokio::test]
    async fn upload_network_error_includes_reqwest_cause() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let error = client
            .upload_asset_with_live_photo(
                asset_path.to_str().unwrap(),
                "file-hash",
                None,
                None,
                SidecarDirectoryCache::new(),
            )
            .await
            .unwrap_err();
        std::fs::remove_file(asset_path).unwrap();

        let prefix = "Network error during upload: ";
        assert!(error.starts_with(prefix));
        assert!(error.len() > prefix.len());
    }

    #[tokio::test]
    async fn upload_request_uses_injected_timeout_when_server_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_headers(&mut stream).await;
            std::future::pending::<()>().await;
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let payload = super::upload_form(
            asset_path.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        let error = client
            .post_upload_form(
                payload.form,
                "file-hash",
                std::time::Duration::from_millis(50),
            )
            .await
            .unwrap_err();
        std::fs::remove_file(asset_path).unwrap();

        assert!(error.starts_with("Network error during upload: "));
    }

    #[tokio::test]
    async fn upload_request_succeeds_immediately_with_large_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"id\":\"large\"}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let asset_path = temporary_file(b"image data");
        let payload = super::upload_form(
            asset_path.to_str().unwrap(),
            None,
            None,
            SidecarDirectoryCache::new(),
        )
        .await
        .unwrap();
        let response = client
            .post_upload_form(
                payload.form,
                "file-hash",
                upload_timeout(10 * GIB_BYTES as u64),
            )
            .await
            .unwrap();
        std::fs::remove_file(asset_path).unwrap();

        assert_eq!(response, "large");
    }

    #[tokio::test]
    async fn validates_login_with_current_user_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 31\r\nConnection: close\r\n\r\n{\"name\":\"Test User\",\"email\":\"\"}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        client.validate_connection().await.unwrap();
        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.starts_with("GET /api/users/me HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert!(!request.contains("cf-access-client-id"));
        assert!(!request.contains("cf-access-client-secret"));
        assert!(!request.contains("bulk-upload-check"));
    }

    #[tokio::test]
    async fn mock_immich_rejects_malformed_bulk_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let error = client
            .check_assets_exist(vec!["file".to_string()])
            .await
            .unwrap_err();
        assert!(error.is_retryable());
        assert!(error.to_string().contains("missing results array"));
    }

    #[tokio::test]
    async fn current_user_info_parses_profile_image_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            let body = r#"{"id":"user-uuid-123","name":"Meyer","email":"meyer@example.com","profileImagePath":"profile/user-uuid-123.jpg"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let info = client.current_user_info().await.unwrap();
        assert_eq!(info.id, "user-uuid-123");
        assert_eq!(info.display_name, "Meyer");
        assert!(info.has_profile_image);
    }

    #[tokio::test]
    async fn fetches_profile_image_bytes_with_auth_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            let fake_image = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                fake_image.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(fake_image).await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let (bytes, mime) = client.get_profile_image("user-uuid-123").await.unwrap();
        assert_eq!(bytes, b"\xFF\xD8\xFF\xE0\x00\x10JFIF");
        assert_eq!(mime.as_deref(), Some("image/jpeg"));

        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.starts_with("GET /api/users/user-uuid-123/profile-image HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
    }

    #[tokio::test]
    async fn get_profile_image_rejects_unsafe_user_ids_before_requesting() {
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse("http://127.0.0.1:9999/api/").unwrap(),
            api_key: "test-key".to_string(),
        };

        for user_id in [
            "", "   ", "../other", "user..id", "user/id", "user\\id", "user?x", "user#x",
            "user\tid",
        ] {
            assert_eq!(
                client.get_profile_image(user_id).await.unwrap_err(),
                "Invalid user ID for API path"
            );
        }
    }

    #[tokio::test]
    async fn fetches_person_thumbnail_with_authentication() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            let image = b"\xFF\xD8\xFF\xE0person-thumbnail";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                image.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(image).await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };

        assert_eq!(
            client.get_person_thumbnail("person-1").await.unwrap(),
            b"\xFF\xD8\xFF\xE0person-thumbnail"
        );
        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.starts_with("GET /api/people/person-1/thumbnail HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
    }

    #[tokio::test]
    async fn fetches_profile_image_with_cloudflare_access_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests_tx
                .send(read_http_request(&mut stream).await)
                .await
                .unwrap();
            let fake_image = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                fake_image.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(fake_image).await.unwrap();
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            "test-key".to_string(),
            true,
            Some("cf-id".to_string()),
            Some("cf-secret".to_string()),
        )
        .unwrap();
        let (bytes, mime) = client.get_profile_image("user-uuid-123").await.unwrap();
        assert_eq!(bytes, b"\xFF\xD8\xFF\xE0\x00\x10JFIF");
        assert_eq!(mime.as_deref(), Some("image/jpeg"));

        let request = String::from_utf8(requests_rx.recv().await.unwrap()).unwrap();
        assert!(request.starts_with("GET /api/users/user-uuid-123/profile-image HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert!(request.contains("cf-access-client-id: cf-id"));
        assert!(request.contains("cf-access-client-secret: cf-secret"));
    }

    #[tokio::test]
    async fn filters_non_image_content_type() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            let fake_body = b"not an image";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                fake_body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(fake_body).await.unwrap();
        });
        let client = ImmichClient {
            client: Client::new(),
            server_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
            api_key: "test-key".to_string(),
        };
        let (bytes, mime) = client.get_profile_image("user-123").await.unwrap();
        assert_eq!(bytes, b"not an image");
        assert_eq!(mime, None);
    }
}
