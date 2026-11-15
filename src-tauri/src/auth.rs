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

impl AuthConfig {
    /// Stable identity of the account these credentials belong to; the one place it is derived.
    pub fn account_id(&self) -> String {
        crate::audit::actor_id(&self.server_url, &self.api_key)
    }

    /// Makes an error safe for logs, errors and UI events by replacing every credential.
    ///
    /// The server URL is also replaced in the form the HTTP client reports it
    /// (trimmed and normalized), so a mixed-case or padded stored URL cannot slip through.
    pub fn redact(&self, error: &str) -> String {
        let mut server_urls = vec![self.server_url.clone(), self.server_url.trim().to_string()];
        if let Ok(normalized) = url::Url::parse(self.server_url.trim()) {
            server_urls.push(normalized.as_str().to_string());
            server_urls.push(normalized.as_str().trim_end_matches('/').to_string());
        }
        server_urls.sort_by_key(|url| std::cmp::Reverse(url.len()));
        server_urls.dedup();
        let mut secrets: Vec<Option<&str>> =
            server_urls.iter().map(|url| Some(url.as_str())).collect();
        secrets.extend([
            Some(self.api_key.as_str()),
            self.cloudflare_client_id.as_deref(),
            self.cloudflare_client_secret.as_deref(),
        ]);
        crate::audit::audit_safe_error(error, &secrets)
    }
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
    use super::{parse_stored_credentials, AuthConfig};

    #[test]
    fn redacts_all_credentials_from_diagnostic_errors() {
        let credentials = AuthConfig {
            server_url: "https://immich.example.test".to_string(),
            api_key: "api-key-secret".to_string(),
            allow_local_http: false,
            cloudflare_client_id: Some("cloudflare-client-id".to_string()),
            cloudflare_client_secret: Some("cloudflare-client-secret".to_string()),
        };
        let safe_error = credentials.redact(
            "failed https://immich.example.test api-key-secret cloudflare-client-id cloudflare-client-secret",
        );

        for secret in [
            "https://immich.example.test",
            "api-key-secret",
            "cloudflare-client-id",
            "cloudflare-client-secret",
        ] {
            assert!(!safe_error.contains(secret));
        }
    }

    #[test]
    fn unreadable_stored_credentials_do_not_echo_stored_values() {
        let error = parse_stored_credentials(
            r#"{"server_url":"https://immich.example","api_key":"secret-key","allow_local_http":"secret-flag"}"#,
        )
        .unwrap_err();

        assert_eq!(error, "Stored credentials are unreadable");
    }

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

/// Parses the keyring payload without surfacing serde's message, which can echo stored values.
fn parse_stored_credentials(json: &str) -> Result<AuthConfig, String> {
    serde_json::from_str(json).map_err(|_| "Stored credentials are unreadable".to_string())
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
        Ok(json) => Ok(Some(parse_stored_credentials(&json)?)),
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
