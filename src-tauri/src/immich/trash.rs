//! Restore of Trashed Assets: the only Immich write the Trash Timeline performs.

use super::client::ImmichClient;
use super::ids::RemoteAssetId;
use super::response::{
    read_response_body_limited, request_failure_code, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The requested Trashed Assets Immich confirmed as restored; every other requested asset is still in the trash.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrashRestoreResult {
    pub restored_asset_ids: Vec<String>,
}

#[derive(Deserialize)]
struct TrashRestoreResponse {
    count: usize,
}

/// What one `POST trash/restore/assets` call told us.
enum RestoreCall {
    /// A 200 with a readable `count`.
    Counted(usize),
    /// A 400: Immich refuses the whole request, for instance because one ID no longer exists.
    Rejected,
}

impl ImmichClient {
    /// Restores Trashed Assets, preferring one bulk request and falling back to one request per asset.
    ///
    /// Observed on Immich v3.4.1 (one server version only): `POST trash/restore/assets` with
    /// `{"ids": [...]}` answers 200 `{"count": n}`, where `count` is the number of requested IDs
    /// that exist and are accessible, not the number actually restored (IDs that were already out
    /// of the trash count too). One unknown or unowned ID fails the whole request with 400 and
    /// restores nothing, and there is no per-asset result. So a batch is trusted only when `count`
    /// equals the number requested; a 400 or any other `count` makes each asset be restored on its
    /// own, where 200 with `count` 1 means restored and 400 means that asset failed. A stale ID
    /// therefore cannot block the others. The permission is `asset.delete`.
    ///
    /// Failures abort the whole call with a credential-free code: `trash_restore_authentication_required`
    /// (401), `trash_restore_permission_denied:asset.delete` (403) and `trash_restore_unavailable`
    /// otherwise (including transport errors); the response body is never forwarded. A restore is
    /// idempotent, so retrying after an abort is safe.
    pub async fn restore_trashed_assets(
        &self,
        asset_ids: &[RemoteAssetId],
    ) -> Result<TrashRestoreResult, String> {
        let restored_asset_ids = Self::run_bulk_with_per_asset_fallback(
            asset_ids,
            "Asset IDs for restore must be unique",
            |ids| async move {
                Ok(matches!(
                    self.post_restore(ids).await?,
                    RestoreCall::Counted(count) if count == ids.len()
                ))
            },
            |asset_id| async move {
                Ok(matches!(
                    self.post_restore(std::slice::from_ref(asset_id)).await?,
                    RestoreCall::Counted(1)
                ))
            },
        )
        .await?;
        Ok(TrashRestoreResult { restored_asset_ids })
    }

    async fn post_restore(&self, asset_ids: &[RemoteAssetId]) -> Result<RestoreCall, String> {
        let response = self
            .request(Method::POST, "trash/restore/assets")
            .json(&json!({ "ids": asset_ids }))
            .send()
            .await
            .map_err(|_| "trash_restore_unavailable".to_string())?;
        let status = response.status();
        if status == StatusCode::BAD_REQUEST {
            return Ok(RestoreCall::Rejected);
        }
        if !status.is_success() {
            return Err(request_failure_code(
                "trash_restore",
                status,
                "asset.delete",
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let restored: TrashRestoreResponse = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid trash restore response: {error}"))?;
        Ok(RestoreCall::Counted(restored.count))
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
            }
        );
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /api/trash/restore/assets HTTP/1.1"));
        assert!(requests[0].contains("x-api-key: test-key"));
        assert!(requests[0].ends_with(r#"{"ids":["trash-1","trash-2"]}"#));
    }

    #[tokio::test]
    async fn restores_each_asset_individually_when_the_batch_is_rejected() {
        let (url, server) = serve_pages(vec![
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.delete access"}"#,
            ),
            ("200 OK", r#"{"count":1}"#),
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.delete access"}"#,
            ),
            ("200 OK", r#"{"count":1}"#),
        ])
        .await;

        let result = test_client(url, "test-key")
            .restore_trashed_assets(&ids(&["trash-1", "gone-2", "trash-3"]))
            .await
            .unwrap();

        assert_eq!(result.restored_asset_ids, vec!["trash-1", "trash-3"]);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].ends_with(r#"{"ids":["trash-1","gone-2","trash-3"]}"#));
        for (request, id) in requests[1..].iter().zip(["trash-1", "gone-2", "trash-3"]) {
            assert!(request.starts_with("POST /api/trash/restore/assets HTTP/1.1"));
            assert!(request.ends_with(&format!(r#"{{"ids":["{id}"]}}"#)));
        }
    }

    #[tokio::test]
    async fn restores_each_asset_individually_when_the_count_differs_from_the_request() {
        for batch_count in [0, 1, 5] {
            let (url, server) = serve_pages(vec![
                ("200 OK", format!(r#"{{"count":{batch_count}}}"#)),
                ("200 OK", r#"{"count":1}"#.to_string()),
                ("200 OK", r#"{"count":0}"#.to_string()),
            ])
            .await;

            let result = test_client(url, "test-key")
                .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
                .await
                .unwrap();

            assert_eq!(result.restored_asset_ids, vec!["trash-1"]);
            assert_eq!(server.await.unwrap().len(), 3);
        }
    }

    #[tokio::test]
    async fn a_single_asset_rejected_by_the_batch_call_is_not_requested_twice() {
        for (status, body) in [
            ("400 Bad Request", r#"{"message":"Not found"}"#),
            ("200 OK", r#"{"count":0}"#),
        ] {
            let (url, server) = serve_pages(vec![(status, body)]).await;

            let result = test_client(url, "test-key")
                .restore_trashed_assets(&ids(&["gone-1"]))
                .await
                .unwrap();

            assert!(result.restored_asset_ids.is_empty());
            assert_eq!(server.await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn aborts_the_fallback_on_credential_or_server_failures() {
        for (status, code) in [
            ("401 Unauthorized", "trash_restore_authentication_required"),
            (
                "403 Forbidden",
                "trash_restore_permission_denied:asset.delete",
            ),
            ("500 Internal Server Error", "trash_restore_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![
                ("400 Bad Request", "{}"),
                ("200 OK", r#"{"count":1}"#),
                (status, "internal secret stacktrace"),
            ])
            .await;

            let error = test_client(url, "restore-secret-key")
                .restore_trashed_assets(&ids(&["trash-1", "trash-2", "trash-3"]))
                .await
                .unwrap_err();

            assert_eq!(error, code);
            assert_eq!(server.await.unwrap().len(), 3);
        }
    }

    #[tokio::test]
    async fn aborts_the_fallback_on_a_malformed_individual_result() {
        let (url, server) =
            serve_pages(vec![("400 Bad Request", "{}"), ("200 OK", "{not-json")]).await;

        let error = test_client(url, "restore-secret-key")
            .restore_trashed_assets(&ids(&["trash-1", "trash-2"]))
            .await
            .unwrap_err();

        assert!(
            error.starts_with("Invalid trash restore response:"),
            "{error}"
        );
        assert_eq!(server.await.unwrap().len(), 2);
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
    async fn rejects_a_malformed_batch_result() {
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
                restored_asset_ids: vec![]
            }
        );
    }
}
