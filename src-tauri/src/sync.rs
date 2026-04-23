use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json::json;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{self, Read};

pub fn calculate_hash(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let result = hasher.finalize();
    Ok(general_purpose::STANDARD.encode(result))
}

pub struct ImmichClient {
    client: Client,
    server_url: String,
    api_key: String,
}

impl ImmichClient {
    pub fn new(server_url: String, api_key: String) -> Self {
        let mut base_url = server_url.trim_end_matches('/').to_string();
        if !base_url.ends_with("/api") {
            base_url.push_str("/api");
        }
        Self {
            client: Client::new(),
            server_url: base_url,
            api_key,
        }
    }

    pub async fn check_assets_exist(&self, hashes: Vec<String>) -> Result<Vec<String>, String> {
        let url = format!("{}/assets/bulk-upload-check", self.server_url);
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&json!({ "checksums": hashes }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        let mut existing = Vec::new();
        if let Some(results) = data.get("results") {
            if let Some(arr) = results.as_array() {
                for item in arr {
                    if let (Some(hash), Some(action)) = (item.get("checksum"), item.get("action")) {
                        if action.as_str() == Some("reject") {
                            existing.push(hash.as_str().unwrap_or_default().to_string());
                        }
                    }
                }
            }
        }
        Ok(existing)
    }

    pub async fn upload_asset(&self, path: &str, device_id: &str) -> Result<String, String> {
        let url = format!("{}/assets", self.server_url);
        let path_buf = std::path::PathBuf::from(path);
        let file_name = path_buf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let created_at = metadata.created().unwrap_or_else(|_| metadata.modified().unwrap());
        let modified_at = metadata.modified().unwrap_or(created_at);
        
        let created_at_iso = chrono::DateTime::<chrono::Utc>::from(created_at).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let modified_at_iso = chrono::DateTime::<chrono::Utc>::from(modified_at).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

        let mut form = reqwest::multipart::Form::new()
            .text("deviceAssetId", path.to_string())
            .text("deviceId", device_id.to_string())
            .text("fileCreatedAt", created_at_iso)
            .text("fileModifiedAt", modified_at_iso)
            .part("assetData", reqwest::multipart::Part::bytes(std::fs::read(path).map_err(|e| e.to_string())?)
                .file_name(file_name.to_string()));

        // Check for sidecar file (.xmp)
        let sidecar_path = path_buf.with_extension("xmp");
        if sidecar_path.exists() {
            let sidecar_name = sidecar_path.file_name().and_then(|n| n.to_str()).unwrap_or("file.xmp");
            form = form.part("sidecarData", reqwest::multipart::Part::bytes(std::fs::read(&sidecar_path).map_err(|e| e.to_string())?)
                .file_name(sidecar_name.to_string()));
        }

        let hash = calculate_hash(path).map_err(|e| e.to_string())?;
        
        let response = self.client.post(&url)
            .header("x-api-key", &self.api_key)
            .header("x-immich-checksum", hash)
            .timeout(std::time::Duration::from_secs(60))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
            Ok(data["id"].as_str().unwrap_or("").to_string())
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            Err(format!("Upload failed: {} - {}", status, error_text))
        }
    }
}
