//! Bulk existence-check retry handling.

use crate::{
    app::events::{log_to_ui_error, log_to_ui_keyed},
    audit::{self, audit_event, SyncAuditContext},
    immich::{BulkCheckDuplicate, ImmichClient},
};
use serde_json::json;

const BULK_CHECK_MAX_ATTEMPTS: usize = 3;
const BULK_CHECK_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
const _: () = assert!(BULK_CHECK_MAX_ATTEMPTS > 0);

pub(crate) async fn check_assets_exist_with_backoff<R: tauri::Runtime>(
    client: &ImmichClient,
    hashes: Vec<String>,
    app: &tauri::AppHandle<R>,
    audit_context: &SyncAuditContext,
) -> Result<Vec<BulkCheckDuplicate>, String> {
    for attempt in 1..=BULK_CHECK_MAX_ATTEMPTS {
        match client.check_assets_exist(hashes.clone()).await {
            Ok(existing) => return Ok(existing),
            Err(error) if !error.is_retryable() => {
                let safe_error = audit_context.safe_error(&error.to_string());
                let message = format!("Bulk check cannot be retried; aborting sync: {safe_error}");
                log_to_ui_error(
                    app,
                    "ERROR",
                    &message,
                    "log_bulk_check_cannot_retry",
                    &safe_error,
                );
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Failure,
                        audit::Severity::Error,
                        "Bulk check cannot be retried",
                        json!({
                            "failure_reason": safe_error,
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) if attempt == BULK_CHECK_MAX_ATTEMPTS => {
                let safe_error = audit_context.safe_error(&error.to_string());
                let message = format!(
                    "Bulk check failed after {} attempts; aborting sync: {}",
                    attempt, safe_error
                );
                let mut params = std::collections::HashMap::new();
                params.insert("attempts".to_string(), attempt.to_string());
                params.insert("error".to_string(), safe_error.clone());
                log_to_ui_keyed(
                    app,
                    "ERROR",
                    &message,
                    "log_bulk_check_failed_max_attempts",
                    params,
                );
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Failure,
                        audit::Severity::Error,
                        "Bulk check failed",
                        json!({
                            "failure_reason": safe_error,
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) => {
                let safe_error = audit_context.safe_error(&error.to_string());
                let delay = BULK_CHECK_INITIAL_BACKOFF * (1 << (attempt - 1));
                let mut params = std::collections::HashMap::new();
                params.insert("attempt".to_string(), attempt.to_string());
                params.insert("max".to_string(), BULK_CHECK_MAX_ATTEMPTS.to_string());
                params.insert("error".to_string(), safe_error.clone());
                params.insert("seconds".to_string(), delay.as_secs().to_string());
                log_to_ui_keyed(
                    app,
                    "WARN",
                    &format!(
                        "Bulk check attempt {}/{} failed: {}. Retrying in {} seconds.",
                        attempt,
                        BULK_CHECK_MAX_ATTEMPTS,
                        safe_error,
                        delay.as_secs()
                    ),
                    "log_bulk_check_retry",
                    params,
                );
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Info,
                        audit::Severity::Warn,
                        "Bulk check retry scheduled",
                        json!({
                            "failure_reason": safe_error,
                            "attempt": attempt,
                        }),
                    ),
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    unreachable!("the final bulk-check attempt always returns")
}

pub(crate) use crate::media::scanner::prepare_failed_sync_retry;

#[cfg(test)]
mod tests {
    use super::check_assets_exist_with_backoff;
    use crate::{audit::SyncAuditContext, auth::AuthConfig, immich::ImmichClient};
    use std::sync::{Arc, Mutex};
    use tauri::Listener;
    use tokio::{io::AsyncWriteExt, net::TcpListener};

    #[tokio::test]
    async fn redacts_credentials_from_non_retryable_bulk_check_ui_log() {
        let secret = "api-key-secret";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\nrequest rejected: {secret}",
                        "request rejected: ".len() + secret.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let app = tauri::test::mock_app();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let captured_logs = Arc::clone(&logs);
        app.handle().listen("log-message", move |event| {
            captured_logs
                .lock()
                .unwrap()
                .push(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap());
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            secret.to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let audit_context = SyncAuditContext::from_credentials(
            &AuthConfig {
                server_url: format!("http://{address}"),
                api_key: secret.to_string(),
                allow_local_http: true,
                cloudflare_client_id: None,
                cloudflare_client_secret: None,
            },
            "manual_sync",
        );

        check_assets_exist_with_backoff(
            &client,
            vec!["hash".to_string()],
            app.handle(),
            &audit_context,
        )
        .await
        .unwrap_err();

        let logs = logs.lock().unwrap();
        let log = logs
            .iter()
            .find(|log| log["key"] == "log_bulk_check_cannot_retry")
            .unwrap();
        assert!(!serde_json::to_string(log).unwrap().contains(secret));
    }

    #[tokio::test]
    async fn redacts_credentials_from_retry_and_final_bulk_check_ui_logs() {
        let secret = "api-key-secret";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\nrequest rejected: {secret}",
                            "request rejected: ".len() + secret.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let app = tauri::test::mock_app();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let captured_logs = Arc::clone(&logs);
        app.handle().listen("log-message", move |event| {
            captured_logs
                .lock()
                .unwrap()
                .push(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap());
        });
        let client = ImmichClient::new(
            format!("http://{address}"),
            secret.to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let audit_context = SyncAuditContext::from_credentials(
            &AuthConfig {
                server_url: format!("http://{address}"),
                api_key: secret.to_string(),
                allow_local_http: true,
                cloudflare_client_id: None,
                cloudflare_client_secret: None,
            },
            "manual_sync",
        );

        check_assets_exist_with_backoff(
            &client,
            vec!["hash".to_string()],
            app.handle(),
            &audit_context,
        )
        .await
        .unwrap_err();

        let logs = logs.lock().unwrap();
        for key in ["log_bulk_check_retry", "log_bulk_check_failed_max_attempts"] {
            let log = logs.iter().find(|log| log["key"] == key).unwrap();
            assert!(!serde_json::to_string(log).unwrap().contains(secret));
        }
    }
}
