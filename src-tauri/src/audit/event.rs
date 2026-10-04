use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

const MAX_MESSAGE_BYTES: usize = 2_048;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    Started,
    Skipped,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    schema_version: u8,
    timestamp: String,
    event_id: String,
    operation_id: String,
    event_type: String,
    outcome: Outcome,
    severity: Severity,
    actor_id: Option<String>,
    source: String,
    message: String,
    details: Value,
}

impl AuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: impl Into<String>,
        event_type: impl Into<String>,
        outcome: Outcome,
        severity: Severity,
        actor_id: Option<String>,
        source: impl Into<String>,
        message: impl AsRef<str>,
        details: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            event_id: Uuid::new_v4().to_string(),
            operation_id: operation_id.into(),
            event_type: event_type.into(),
            outcome,
            severity,
            actor_id,
            source: source.into(),
            message: bounded_message(message.as_ref()),
            details,
        }
    }

    pub fn operation_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.outcome, Outcome::Success | Outcome::Failure)
    }

    #[cfg(test)]
    pub(crate) fn set_message_for_test(&mut self, message: String) {
        self.message = message;
    }
}

pub fn actor_id(server_url: &str, api_key: &str) -> String {
    let normalized_url = server_url.trim().trim_end_matches('/').to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(b"lymic.audit.actor.v1\0");
    hasher.update(normalized_url.as_bytes());
    hasher.update(b"\0");
    hasher.update(api_key.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Clone)]
pub struct SyncAuditContext {
    operation_id: String,
    actor_id: Option<String>,
    source: &'static str,
    server_url: String,
    api_key: String,
}

impl SyncAuditContext {
    pub fn from_credentials(server_url: &str, api_key: &str, source: &'static str) -> Self {
        Self {
            operation_id: AuditEvent::operation_id(),
            actor_id: Some(actor_id(server_url, api_key)),
            source,
            server_url: server_url.to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn event(
        &self,
        event_type: &str,
        outcome: Outcome,
        severity: Severity,
        message: &str,
        details: Value,
    ) -> AuditEvent {
        AuditEvent::new(
            &self.operation_id,
            event_type,
            outcome,
            severity,
            self.actor_id.clone(),
            self.source,
            message,
            details,
        )
    }

    pub fn safe_error(&self, error: &str) -> String {
        audit_safe_error(error, Some(&self.server_url), Some(&self.api_key))
    }
}

pub fn audit_safe_error(error: &str, server_url: Option<&str>, api_key: Option<&str>) -> String {
    let mut redacted = error.replace(['\r', '\n'], " ");
    for secret in [server_url, api_key]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
    {
        redacted = redacted.replace(secret, "[redacted]");
    }
    redacted
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512)
        .collect()
}

pub fn upload_details(
    asset: &crate::media::model::SyncAsset,
    checksums: &crate::media::model::Checksums,
    file_operation_id: &str,
    upload_started_at: &str,
    upload_finished_at: Option<String>,
    remote_asset_id: Option<&str>,
    failure_reason: Option<String>,
) -> Value {
    let mut details = json!({
        "file_operation_id": file_operation_id,
        "local_path": asset.path,
        "file_name": Path::new(&asset.path).file_name().and_then(|name| name.to_str()).unwrap_or("file"),
        "file_size_bytes": asset.size,
        "checksums": {
            "md5": checksums.md5_hex,
            "sha256": checksums.sha256_hex,
            "sha1_base64": checksums.sha1_base64,
        },
        "upload_started_at": upload_started_at,
    });
    if let Some(finished_at) = upload_finished_at {
        details["upload_finished_at"] = json!(finished_at);
    }
    if let Some(remote_id) = remote_asset_id {
        details["remote_asset_id"] = json!(remote_id);
    }
    if let Some(reason) = failure_reason {
        details["failure_reason"] = json!(reason);
    }
    details
}

fn bounded_message(message: &str) -> String {
    let single_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.len() <= MAX_MESSAGE_BYTES {
        return single_line;
    }
    let mut end = MAX_MESSAGE_BYTES;
    while !single_line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &single_line[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_schema_without_secret_input() {
        let event = AuditEvent::new(
            "operation",
            "file.upload.completed",
            Outcome::Success,
            Severity::Info,
            Some(actor_id("https://example.test/", "secret")),
            "manual_sync",
            "Upload completed",
            json!({}),
        );
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert!(value["timestamp"].as_str().unwrap().ends_with('Z'));
        assert!(!value.to_string().contains("secret"));
        assert_eq!(
            value["actor_id"].as_str().unwrap().len(),
            "sha256:".len() + 64
        );
    }

    #[test]
    fn actor_is_stable_and_domain_separated() {
        assert_eq!(
            actor_id("https://EXAMPLE.test/", "key"),
            actor_id("https://example.test", "key")
        );
        assert_ne!(
            actor_id("https://example.test", "key"),
            actor_id("https://other.test", "key")
        );
    }
}
