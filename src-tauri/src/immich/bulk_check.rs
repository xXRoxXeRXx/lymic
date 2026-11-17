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
    let detail = match response_error_detail(response).await {
        Ok(detail) => detail,
        Err(error) => {
            return BulkCheckError::Retryable(format!(
                "Bulk-check error response could not be read ({}): {}",
                status, error
            ));
        }
    };
    match bulk_check_status_error(status) {
        BulkCheckError::Retryable(message) => {
            BulkCheckError::Retryable(format!("{}{}", message, detail))
        }
        BulkCheckError::NonRetryable(message) => {
            BulkCheckError::NonRetryable(format!("{}{}", message, detail))
        }
    }
}

/// The Remote Asset that Immich recognized a Duplicate as, and whether it is a Trashed Asset
/// (which makes the Duplicate a Trashed Duplicate, ADR-0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DuplicateRemoteAsset {
    pub id: super::ids::RemoteAssetId,
    pub is_trashed: bool,
}

impl DuplicateRemoteAsset {
    /// The Remote Asset `id`, trashed when the Immich response `item` naming it says
    /// `isTrashed: true` (absent means false).
    pub(super) fn new(id: super::ids::RemoteAssetId, item: &serde_json::Value) -> Self {
        Self {
            id,
            is_trashed: item
                .get("isTrashed")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BulkCheckDuplicate {
    pub hash: String,
    /// Absent when the bulk check named no asset ID; it is then found by checksum.
    pub remote_asset: Option<DuplicateRemoteAsset>,
}

pub(super) fn parse_bulk_check_response(
    data: &serde_json::Value,
    requested_hashes: &[String],
) -> Result<Vec<BulkCheckDuplicate>, String> {
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
            ("reject", None) | ("reject", Some("duplicate")) => {
                let remote_asset = result
                    .get("assetId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|asset_id| {
                        DuplicateRemoteAsset::new(super::ids::RemoteAssetId::new(asset_id), result)
                    });
                existing.push(BulkCheckDuplicate {
                    hash: id.to_string(),
                    remote_asset,
                });
            }
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
    use super::{
        bulk_check_status_error, parse_bulk_check_response, BulkCheckDuplicate,
        DuplicateRemoteAsset,
    };
    use crate::immich::RemoteAssetId;
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn bulk_check_extracts_duplicate_asset_id_when_present() {
        let hashes = vec!["first".to_string(), "second".to_string()];
        let response = json!({
            "results": [
                { "id": "first", "action": "reject", "reason": "duplicate", "assetId": "asset-uuid-1" },
                { "id": "second", "action": "reject", "reason": "duplicate" }
            ]
        });
        let duplicates = parse_bulk_check_response(&response, &hashes).unwrap();
        assert_eq!(
            duplicates,
            vec![
                BulkCheckDuplicate {
                    hash: "first".to_string(),
                    remote_asset: Some(DuplicateRemoteAsset {
                        id: RemoteAssetId::new("asset-uuid-1"),
                        is_trashed: false,
                    }),
                },
                BulkCheckDuplicate {
                    hash: "second".to_string(),
                    remote_asset: None,
                }
            ]
        );
    }

    #[test]
    fn bulk_check_requires_a_complete_valid_response() {
        let hashes = vec!["first".to_string(), "second".to_string()];
        let valid = json!({ "results": [{ "id": "first", "action": "reject" }, { "id": "second", "action": "accept" }] });
        assert_eq!(
            parse_bulk_check_response(&valid, &hashes).unwrap(),
            vec![BulkCheckDuplicate {
                hash: "first".to_string(),
                remote_asset: None,
            }]
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

    #[tokio::test]
    async fn bulk_check_reports_a_duplicate_whose_remote_asset_is_in_the_trash() {
        // Shape observed on Immich v3.4.1: a trashed Remote Asset is still a duplicate,
        // with an extra `isTrashed: true`.
        let (url, request) = crate::immich::test_support::serve_once(
            "200 OK",
            r#"{"results":[
                {"id":"trashed-hash","action":"reject","reason":"duplicate","assetId":"trashed-id","isTrashed":true},
                {"id":"live-hash","action":"reject","reason":"duplicate","assetId":"live-id","isTrashed":false},
                {"id":"new-hash","action":"accept"}
            ]}"#,
        )
        .await;
        let client = crate::immich::test_support::test_client(url, "test-key");

        let duplicates = client
            .check_assets_exist(vec![
                "trashed-hash".to_string(),
                "live-hash".to_string(),
                "new-hash".to_string(),
            ])
            .await
            .unwrap();

        assert!(request
            .await
            .unwrap()
            .starts_with("POST /api/assets/bulk-upload-check HTTP/1.1"));
        assert_eq!(
            duplicates,
            vec![
                BulkCheckDuplicate {
                    hash: "trashed-hash".to_string(),
                    remote_asset: Some(DuplicateRemoteAsset {
                        id: RemoteAssetId::new("trashed-id"),
                        is_trashed: true,
                    }),
                },
                BulkCheckDuplicate {
                    hash: "live-hash".to_string(),
                    remote_asset: Some(DuplicateRemoteAsset {
                        id: RemoteAssetId::new("live-id"),
                        is_trashed: false,
                    }),
                },
            ]
        );
    }

    #[test]
    fn authentication_errors_are_not_retryable() {
        assert!(!bulk_check_status_error(StatusCode::UNAUTHORIZED).is_retryable());
        assert!(!bulk_check_status_error(StatusCode::FORBIDDEN).is_retryable());
        assert!(bulk_check_status_error(StatusCode::INTERNAL_SERVER_ERROR).is_retryable());
    }
}
