//! Bulk existence-check retry handling.

use crate::{
    app::events::{log_to_ui_error, log_to_ui_keyed},
    audit::{self, audit_event, SyncAuditContext},
    immich::ImmichClient,
};
use serde_json::json;

const BULK_CHECK_MAX_ATTEMPTS: usize = 3;
const BULK_CHECK_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
const _: () = assert!(BULK_CHECK_MAX_ATTEMPTS > 0);

pub(crate) async fn check_assets_exist_with_backoff(
    client: &ImmichClient,
    hashes: Vec<String>,
    app: &tauri::AppHandle,
    audit_context: &SyncAuditContext,
) -> Result<Vec<String>, String> {
    for attempt in 1..=BULK_CHECK_MAX_ATTEMPTS {
        match client.check_assets_exist(hashes.clone()).await {
            Ok(existing) => return Ok(existing),
            Err(error) if !error.is_retryable() => {
                let message = format!("Bulk check cannot be retried; aborting sync: {}", error);
                log_to_ui_error(
                    app,
                    "ERROR",
                    &message,
                    "log_bulk_check_cannot_retry",
                    &error.to_string(),
                );
                audit_event(
                    app,
                    audit_context.event(
                        "file.bulk_check.failed",
                        audit::Outcome::Failure,
                        audit::Severity::Error,
                        "Bulk check cannot be retried",
                        json!({
                            "failure_reason": audit_context.safe_error(&error.to_string()),
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) if attempt == BULK_CHECK_MAX_ATTEMPTS => {
                let message = format!(
                    "Bulk check failed after {} attempts; aborting sync: {}",
                    attempt, error
                );
                let mut params = std::collections::HashMap::new();
                params.insert("attempts".to_string(), attempt.to_string());
                params.insert("error".to_string(), error.to_string());
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
                            "failure_reason": audit_context.safe_error(&error.to_string()),
                            "attempt": attempt,
                        }),
                    ),
                );
                return Err(message);
            }
            Err(error) => {
                let delay = BULK_CHECK_INITIAL_BACKOFF * (1 << (attempt - 1));
                let mut params = std::collections::HashMap::new();
                params.insert("attempt".to_string(), attempt.to_string());
                params.insert("max".to_string(), BULK_CHECK_MAX_ATTEMPTS.to_string());
                params.insert("error".to_string(), error.to_string());
                params.insert("seconds".to_string(), delay.as_secs().to_string());
                log_to_ui_keyed(
                    app,
                    "WARN",
                    &format!(
                        "Bulk check attempt {}/{} failed: {}. Retrying in {} seconds.",
                        attempt,
                        BULK_CHECK_MAX_ATTEMPTS,
                        error,
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
                            "failure_reason": audit_context.safe_error(&error.to_string()),
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
