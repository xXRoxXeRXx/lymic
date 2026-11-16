//! Restore of Trashed Assets: the only Immich write the Trash Timeline performs.

use super::client::ImmichClient;
use super::ids::RemoteAssetId;
use super::response::{
    explore_status_error, read_response_body_limited, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

/// Which requested Trashed Assets were restored and which are still in the Immich trash.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrashRestoreResult {
    pub restored_asset_ids: Vec<String>,
    pub failed_asset_ids: Vec<String>,
}

#[derive(Deserialize)]
struct TrashRestoreResponse {
    count: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetTrashState {
    is_trashed: bool,
}

impl ImmichClient {
    /// Restores Trashed Assets with Immich's bulk restore-from-trash call.
    ///
    /// UNVERIFIED against a live Immich server: the call (`POST trash/restore/assets` with
    /// `{"ids": [...]}`), its result shape (`{"count": n}`, the number of assets restored, with no
    /// per-asset detail) and the API key permission it needs (believed to be `asset.delete`).
    /// Because a partial count does not say which assets failed, each requested asset is then read
    /// back and counts as restored only when it no longer reports `isTrashed` (also unverified).
    ///
    /// Failures carry a credential-free code: `trash_restore_authentication_required` (401),
    /// `trash_restore_permission_denied:asset.delete` (403) and `trash_restore_unavailable`
    /// otherwise; the response body is never forwarded.
    pub async fn restore_trashed_assets(
        &self,
        asset_ids: &[RemoteAssetId],
    ) -> Result<TrashRestoreResult, String> {
        if asset_ids.is_empty() {
            return Ok(TrashRestoreResult {
                restored_asset_ids: Vec::new(),
                failed_asset_ids: Vec::new(),
            });
        }
        for asset_id in asset_ids {
            Self::validate_path_segment(asset_id.as_str(), "asset ID")?;
        }
        let unique: HashSet<&str> = asset_ids.iter().map(|id| id.as_str()).collect();
        if unique.len() != asset_ids.len() {
            return Err("Asset IDs for restore must be unique".to_string());
        }

        let response = self
            .request(Method::POST, "trash/restore/assets")
            .json(&json!({ "ids": asset_ids }))
            .send()
            .await
            .map_err(|_| "trash_restore_unavailable".to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(explore_status_error(
                "trash_restore",
                status,
                "asset.delete",
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let restored: TrashRestoreResponse = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid trash restore response: {error}"))?;
        if restored.count > asset_ids.len() {
            return Err("Immich returned an unexpected trash restore result".to_string());
        }

        let all_ids = || asset_ids.iter().map(|id| id.as_str().to_string());
        if restored.count == asset_ids.len() {
            return Ok(TrashRestoreResult {
                restored_asset_ids: all_ids().collect(),
                failed_asset_ids: Vec::new(),
            });
        }
        if restored.count == 0 {
            return Ok(TrashRestoreResult {
                restored_asset_ids: Vec::new(),
                failed_asset_ids: all_ids().collect(),
            });
        }

        let mut result = TrashRestoreResult {
            restored_asset_ids: Vec::new(),
            failed_asset_ids: Vec::new(),
        };
        for asset_id in all_ids() {
            if self.is_asset_out_of_trash(&asset_id).await {
                result.restored_asset_ids.push(asset_id);
            } else {
                result.failed_asset_ids.push(asset_id);
            }
        }
        Ok(result)
    }

    /// True only when Immich confirms the asset is no longer trashed; any doubt counts as still trashed.
    async fn is_asset_out_of_trash(&self, asset_id: &str) -> bool {
        let Ok(response) = self
            .request(Method::GET, &format!("assets/{asset_id}"))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        let Ok(body) = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await
        else {
            return false;
        };
        serde_json::from_slice::<AssetTrashState>(&body).is_ok_and(|state| !state.is_trashed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::test_support::{serve_pages, test_client};

    fn ids(values: &[&str]) -> Vec<RemoteAssetId> {
        values
            .iter()
            .map(|value| RemoteAssetId::new(*value))
            .collect()
    }

    #[tokio::test]
    async fn restores_all_requested_assets_with_one_bulk_request() {
        let (url, server) = serve_pages(vec![("200 OK", r#"{"count":2}"#)]).await;

        let result = test_client(url, "test-key")
            .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
            .await
            .unwrap();

        assert_eq!(
            result,
            TrashRestoreResult {
                restored_asset_ids: vec!["trash-1".into(), "trash-2".into()],
                failed_asset_ids: vec![],
            }
        );
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /api/trash/restore/assets HTTP/1.1"));
        assert!(requests[0].contains("x-api-key: test-key"));
        assert!(requests[0].ends_with(r#"{"ids":["trash-1","trash-2"]}"#));
    }

    #[tokio::test]
    async fn maps_a_zero_count_to_every_asset_failing() {
        let (url, server) = serve_pages(vec![("200 OK", r#"{"count":0}"#)]).await;

        let result = test_client(url, "test-key")
            .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
            .await
            .unwrap();

        assert!(result.restored_asset_ids.is_empty());
        assert_eq!(result.failed_asset_ids, vec!["trash-1", "trash-2"]);
        assert_eq!(server.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn names_the_failed_assets_of_a_partial_count_by_reading_them_back() {
        let (url, server) = serve_pages(vec![
            ("200 OK", r#"{"count":1}"#),
            ("200 OK", r#"{"id":"trash-1","isTrashed":false}"#),
            ("200 OK", r#"{"id":"trash-2","isTrashed":true}"#),
        ])
        .await;

        let result = test_client(url, "test-key")
            .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
            .await
            .unwrap();

        assert_eq!(result.restored_asset_ids, vec!["trash-1"]);
        assert_eq!(result.failed_asset_ids, vec!["trash-2"]);
        let requests = server.await.unwrap();
        assert!(requests[1].starts_with("GET /api/assets/trash-1 HTTP/1.1"));
        assert!(requests[2].starts_with("GET /api/assets/trash-2 HTTP/1.1"));
    }

    #[tokio::test]
    async fn keeps_an_asset_failed_when_its_state_cannot_be_read_back() {
        let (url, _server) = serve_pages(vec![
            ("200 OK", r#"{"count":1}"#),
            ("500 Internal Server Error", "boom"),
            ("200 OK", r#"{"id":"trash-2","isTrashed":false}"#),
        ])
        .await;

        let result = test_client(url, "test-key")
            .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
            .await
            .unwrap();

        assert_eq!(result.restored_asset_ids, vec!["trash-2"]);
        assert_eq!(result.failed_asset_ids, vec!["trash-1"]);
    }

    #[tokio::test]
    async fn classifies_failures_without_leaking_the_body_or_the_api_key() {
        for (status, code) in [
            ("401 Unauthorized", "trash_restore_authentication_required"),
            (
                "403 Forbidden",
                "trash_restore_permission_denied:asset.delete",
            ),
            ("500 Internal Server Error", "trash_restore_unavailable"),
            ("502 Bad Gateway", "trash_restore_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "restore-secret-key")
                .restore_trashed_assets(&ids(&["trash-1"]))
                .await
                .unwrap_err();
            server.await.unwrap();
            assert_eq!(error, code);
        }
    }

    #[tokio::test]
    async fn rejects_malformed_and_impossible_results() {
        for body in ["{not-json", r#"{"restored":1}"#, r#"[{"id":"trash-1"}]"#] {
            let (url, server) = serve_pages(vec![("200 OK", body)]).await;
            let error = test_client(url, "restore-secret-key")
                .restore_trashed_assets(&ids(&["trash-1"]))
                .await
                .unwrap_err();
            server.await.unwrap();
            assert!(
                error.starts_with("Invalid trash restore response:"),
                "{error}"
            );
            assert!(!error.contains("restore-secret-key"));
        }

        let (url, server) = serve_pages(vec![("200 OK", r#"{"count":5}"#)]).await;
        let error = test_client(url, "restore-secret-key")
            .restore_trashed_assets(&ids(&["trash-1"]))
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error, "Immich returned an unexpected trash restore result");
    }

    #[tokio::test]
    async fn rejects_unsafe_or_duplicate_ids_before_requesting() {
        let client = test_client("http://127.0.0.1:9".to_string(), "test-key");
        assert_eq!(
            client
                .restore_trashed_assets(&ids(&["a/b"]))
                .await
                .unwrap_err(),
            "Invalid asset ID for API path"
        );
        assert_eq!(
            client
                .restore_trashed_assets(&ids(&["a", "a"]))
                .await
                .unwrap_err(),
            "Asset IDs for restore must be unique"
        );
        assert_eq!(
            client.restore_trashed_assets(&[]).await.unwrap(),
            TrashRestoreResult {
                restored_asset_ids: vec![],
                failed_asset_ids: vec![]
            }
        );
    }
}
