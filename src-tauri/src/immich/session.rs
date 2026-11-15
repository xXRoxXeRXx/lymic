use super::ImmichClient;
use crate::auth::AuthConfig;
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

type CachedSession = (AuthConfig, Arc<ImmichClient>, String);

fn session_cache() -> &'static Mutex<Option<CachedSession>> {
    static CACHE: OnceLock<Mutex<Option<CachedSession>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Replaces credentials in an Immich error so it is safe for logs, errors and UI events.
fn redact_error(error: &str, credentials: &AuthConfig) -> String {
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

/// An authenticated Immich session obtained from the stored credentials.
///
/// Every error leaving the session is already credential-free, so callers never
/// redact (and cannot forget to).
pub(crate) struct ImmichSession {
    client: Arc<ImmichClient>,
    credentials: AuthConfig,
    account_id: String,
}

impl ImmichSession {
    pub(crate) fn from_stored_credentials() -> Result<Self, String> {
        Self::open(crate::auth::get_credentials()?)
    }

    fn open(credentials: Option<AuthConfig>) -> Result<Self, String> {
        let credentials = credentials.ok_or_else(|| "Not authenticated".to_string())?;
        if let Ok(cache) = session_cache().lock() {
            if let Some((cached_credentials, client, account_id)) = cache.as_ref() {
                if cached_credentials == &credentials {
                    return Ok(Self {
                        client: client.clone(),
                        credentials,
                        account_id: account_id.clone(),
                    });
                }
            }
        }
        let account_id = crate::audit::actor_id(&credentials.server_url, &credentials.api_key);
        let client = Arc::new(
            ImmichClient::new(
                credentials.server_url.clone(),
                credentials.api_key.clone(),
                credentials.allow_local_http,
                credentials.cloudflare_client_id.clone(),
                credentials.cloudflare_client_secret.clone(),
            )
            .map_err(|error| redact_error(&error, &credentials))?,
        );
        if let Ok(mut cache) = session_cache().lock() {
            *cache = Some((credentials.clone(), client.clone(), account_id.clone()));
        }
        Ok(Self {
            client,
            credentials,
            account_id,
        })
    }

    /// Stable identity of the authenticated account, for per-account caches.
    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Runs an Immich operation and returns its error with credentials redacted.
    pub(crate) async fn run<T, F, Fut>(&self, operation: F) -> Result<T, String>
    where
        F: FnOnce(Arc<ImmichClient>) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        operation(self.client.clone())
            .await
            .map_err(|error| redact_error(&error, &self.credentials))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn credentials(server_url: String) -> AuthConfig {
        AuthConfig {
            server_url,
            api_key: "secret-api-key-123".to_string(),
            allow_local_http: true,
            cloudflare_client_id: Some("cf-client-id-456".to_string()),
            cloudflare_client_secret: Some("cf-client-secret-789".to_string()),
        }
    }

    #[test]
    fn reports_not_authenticated_when_no_credentials_are_stored() {
        let error = ImmichSession::open(None).err().unwrap();
        assert_eq!(error, "Not authenticated");
    }

    #[tokio::test]
    async fn errors_from_a_failing_server_never_contain_any_credential() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_url = format!("http://{address}");
        let config = credentials(server_url.clone());
        let body = format!(
            "denied for {} key=secret-api-key-123 id=cf-client-id-456 secret=cf-client-secret-789",
            server_url
        );
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let _ = stream.read(&mut buffer).await.unwrap();
            let response = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let session = ImmichSession::open(Some(config.clone())).unwrap();
        let error = session
            .run(|client| async move { client.get_albums().await })
            .await
            .unwrap_err();

        assert_credential_free(&error, &config);
    }

    #[tokio::test]
    async fn redacts_credentials_an_operation_echoes_into_its_error() {
        let config = credentials("http://127.0.0.1:1".to_string());
        let session = ImmichSession::open(Some(config.clone())).unwrap();
        let echoed = format!(
            "boom {} {} {} {}",
            config.server_url,
            config.api_key,
            config.cloudflare_client_id.as_ref().unwrap(),
            config.cloudflare_client_secret.as_ref().unwrap(),
        );

        let error = session
            .run(|_client| async move { Err::<(), _>(echoed) })
            .await
            .unwrap_err();

        assert!(error.contains("[redacted]"), "{error}");
        assert_credential_free(&error, &config);
    }

    fn assert_credential_free(error: &str, config: &AuthConfig) {
        for secret in [
            &config.server_url,
            &config.api_key,
            config.cloudflare_client_id.as_ref().unwrap(),
            config.cloudflare_client_secret.as_ref().unwrap(),
        ] {
            assert!(!error.contains(secret.as_str()), "{error}");
        }
    }

    #[tokio::test]
    async fn connection_errors_never_contain_the_server_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let config = credentials(format!("http://{address}"));

        let session = ImmichSession::open(Some(config.clone())).unwrap();
        let error = session
            .run(|client| async move { client.get_albums().await })
            .await
            .unwrap_err();

        assert_credential_free(&error, &config);
    }

    #[test]
    fn invalid_stored_credentials_fail_without_leaking_them() {
        let mut config = credentials("http://127.0.0.1:1".to_string());
        config.cloudflare_client_secret = None;
        let error = ImmichSession::open(Some(config.clone())).err().unwrap();
        assert!(!error.contains(&config.api_key));
    }
}
