use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const CREDENTIAL_CACHE_TTL: Duration = Duration::from_secs(2);

type CachedCredentials = Option<(Instant, Option<AuthConfig>)>;

fn credential_cache() -> &'static Mutex<CachedCredentials> {
    static CACHE: OnceLock<Mutex<CachedCredentials>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    pub server_url: String,
    pub api_key: String,
    #[serde(default)]
    pub allow_local_http: bool,
    #[serde(default)]
    pub cloudflare_client_id: Option<String>,
    #[serde(default)]
    pub cloudflare_client_secret: Option<String>,
}

impl fmt::Debug for AuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthConfig")
            .field("server_url", &self.server_url)
            .field("api_key", &"[redacted]")
            .field("cloudflare_client_id", &self.cloudflare_client_id)
            .field(
                "cloudflare_client_secret",
                &self.cloudflare_client_secret.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

const SERVICE_NAME: &str = "lymic-sync";
const ACCOUNT_NAME: &str = "api-credentials";

pub fn store_credentials(
    server_url: &str,
    api_key: &str,
    allow_local_http: bool,
    cloudflare_client_id: Option<String>,
    cloudflare_client_secret: Option<String>,
) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    let config = AuthConfig {
        server_url: server_url.to_string(),
        api_key: api_key.to_string(),
        allow_local_http,
        cloudflare_client_id,
        cloudflare_client_secret,
    };
    let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| e.to_string())?;
    if let Ok(mut cache) = credential_cache().lock() {
        *cache = Some((Instant::now(), Some(config)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AuthConfig;

    #[test]
    fn legacy_credential_json_defaults_cloudflare_access_to_disabled() {
        let credentials: AuthConfig =
            serde_json::from_str(r#"{"server_url":"https://immich.example","api_key":"key"}"#)
                .unwrap();

        assert!(credentials.cloudflare_client_id.is_none());
        assert!(credentials.cloudflare_client_secret.is_none());
        assert!(!credentials.allow_local_http);
    }

    #[test]
    fn serializes_configured_cloudflare_access_pair() {
        let credentials = AuthConfig {
            server_url: "https://immich.example".to_string(),
            api_key: "key".to_string(),
            allow_local_http: true,
            cloudflare_client_id: Some("client-id".to_string()),
            cloudflare_client_secret: Some("client-secret".to_string()),
        };

        let json = serde_json::to_string(&credentials).unwrap();
        assert!(json.contains(r#""cloudflare_client_id":"client-id""#));
        assert!(json.contains(r#""cloudflare_client_secret":"client-secret""#));
        assert!(json.contains(r#""allow_local_http":true"#));
    }

    #[test]
    fn debug_redacts_credentials() {
        let credentials = AuthConfig {
            server_url: "https://immich.example".to_string(),
            api_key: "api-key".to_string(),
            allow_local_http: false,
            cloudflare_client_id: Some("client-id".to_string()),
            cloudflare_client_secret: Some("client-secret".to_string()),
        };

        let debug = format!("{credentials:?}");
        assert!(!debug.contains("api-key"));
        assert!(!debug.contains("client-secret"));
        assert!(debug.contains("api_key: \"[redacted]\""));
        assert!(debug.contains("cloudflare_client_secret: Some(\"[redacted]\")"));
    }
}

pub fn get_credentials() -> Result<Option<AuthConfig>, String> {
    if let Ok(cache) = credential_cache().lock() {
        if let Some((cached_at, ref config)) = *cache {
            if cached_at.elapsed() < CREDENTIAL_CACHE_TTL {
                return Ok(config.clone());
            }
        }
    }
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    let loaded = match entry.get_password() {
        Ok(json) => {
            let config: AuthConfig = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(Some(config))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }?;
    if let Ok(mut cache) = credential_cache().lock() {
        *cache = Some((Instant::now(), loaded.clone()));
    }
    Ok(loaded)
}

pub fn delete_credentials() -> Result<(), String> {
    if let Ok(mut cache) = credential_cache().lock() {
        *cache = None;
    }
    let entry = Entry::new(SERVICE_NAME, ACCOUNT_NAME).map_err(|e| e.to_string())?;
    // NoEntry is the desired end-state; treat it as success.
    match entry.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            if let Ok(mut cache) = credential_cache().lock() {
                *cache = Some((Instant::now(), None));
            }
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}
