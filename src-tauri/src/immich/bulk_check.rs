use super::response::response_error_detail;
use reqwest::{Response, StatusCode};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug)]
pub(crate) enum BulkCheckError {
    Retryable(String),
    NonRetryable(String),
}

impl BulkCheckError {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

impl fmt::Display for BulkCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(message) | Self::NonRetryable(message) => message.fmt(formatter),
        }
    }
}

fn bulk_check_status_error(status: StatusCode) -> BulkCheckError {
    let message = format!("Server error during bulk check ({})", status);
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        BulkCheckError::NonRetryable(message)
    } else {
        BulkCheckError::Retryable(message)
    }
}

pub(super) async fn bulk_check_response_error(response: Response) -> BulkCheckError {
    let status = response.status();
    let detail = response_error_detail(response).await;
    match bulk_check_status_error(status) {
        BulkCheckError::Retryable(message) => {
            BulkCheckError::Retryable(format!("{}{}", message, detail))
        }
        BulkCheckError::NonRetryable(message) => {
            BulkCheckError::NonRetryable(format!("{}{}", message, detail))
        }
    }
}

pub(super) fn parse_bulk_check_response(
    data: &serde_json::Value,
    requested_hashes: &[String],
) -> Result<Vec<String>, String> {
    let results = data
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or("Invalid bulk-check response: missing results array")?;
    if results.len() != requested_hashes.len() {
        return Err(format!(
            "Invalid bulk-check response: expected {} results, received {}",
            requested_hashes.len(),
            results.len()
        ));
    }
    let mut expected: HashMap<&str, usize> = HashMap::new();
    for hash in requested_hashes {
        *expected.entry(hash).or_default() += 1;
    }
    let mut existing = Vec::new();
    for result in results {
        let id = result
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid bulk-check response: result missing string id")?;
        let action = result
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid bulk-check response: result missing string action")?;
        let remaining = expected
            .get_mut(id)
            .ok_or("Invalid bulk-check response: unexpected result id")?;
        if *remaining == 0 {
            return Err("Invalid bulk-check response: duplicate result id".to_string());
        }
        *remaining -= 1;
        match (
            action,
            result.get("reason").and_then(serde_json::Value::as_str),
        ) {
            ("accept", None) | ("accept", Some(_)) => {}
            ("reject", None) | ("reject", Some("duplicate")) => existing.push(id.to_string()),
            ("reject", Some("unsupported-format")) => {
                return Err(format!("Asset rejected as unsupported format: {}", id))
            }
            ("reject", Some(reason)) => {
                return Err(format!(
                    "Asset rejected with unknown reason '{}': {}",
                    reason, id
                ))
            }
            _ => {
                return Err(format!(
                    "Invalid bulk-check response: unknown action '{}'",
                    action
                ))
            }
        }
    }
    if expected.values().any(|remaining| *remaining != 0) {
        return Err("Invalid bulk-check response: response omitted requested assets".to_string());
    }
    Ok(existing)
}

#[cfg(test)]
mod tests {
    use super::{bulk_check_status_error, parse_bulk_check_response};
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn bulk_check_requires_a_complete_valid_response() {
        let hashes = vec!["first".to_string(), "second".to_string()];
        let valid = json!({ "results": [{ "id": "first", "action": "reject" }, { "id": "second", "action": "accept" }] });
        assert_eq!(
            parse_bulk_check_response(&valid, &hashes).unwrap(),
            vec!["first".to_string()]
        );
        for invalid in [
            json!({}),
            json!({ "results": [] }),
            json!({ "results": [{ "id": "first", "action": "accept" }, { "id": "unknown", "action": "accept" }] }),
            json!({ "results": [{ "id": "first", "action": "accept" }, { "id": "first", "action": "reject" }] }),
            json!({ "results": [{ "id": "first", "action": "ignore" }, { "id": "second", "action": "accept" }] }),
        ] {
            assert!(parse_bulk_check_response(&invalid, &hashes).is_err());
        }
        let unsupported = json!({ "results": [{ "id": "first", "action": "reject", "reason": "unsupported-format" }, { "id": "second", "action": "accept" }] });
        assert!(parse_bulk_check_response(&unsupported, &hashes)
            .unwrap_err()
            .contains("unsupported format"));
    }

    #[test]
    fn authentication_errors_are_not_retryable() {
        assert!(!bulk_check_status_error(StatusCode::UNAUTHORIZED).is_retryable());
        assert!(!bulk_check_status_error(StatusCode::FORBIDDEN).is_retryable());
        assert!(bulk_check_status_error(StatusCode::INTERNAL_SERVER_ERROR).is_retryable());
    }
}
