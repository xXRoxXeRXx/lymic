//! The two Immich writes that touch trash state: Restore of Trashed Assets and Move to Trash (ADR-0011).

use super::client::ImmichClient;
use super::ids::RemoteAssetId;
use super::response::{
    read_response_body_limited, request_failure_code, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

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

/// The requested Remote Assets Immich confirmed as moved to the trash; every other requested asset is still in the library.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrashMoveResult {
    pub moved_asset_ids: Vec<String>,
}

/// What one `DELETE assets` call told us.
enum TrashCall {
    /// A 204: every requested asset is in the trash.
    Moved,
    /// A 400: Immich refuses the whole request and changes nothing, for instance because one ID is unknown or not owned.
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
        if asset_ids.is_empty() {
            return Ok(TrashRestoreResult {
                restored_asset_ids: Vec::new(),
            });
        }
        Self::validate_asset_ids(asset_ids, "Asset IDs for restore must be unique")?;

        let batch = self.post_restore(asset_ids).await?;
        if let RestoreCall::Counted(count) = batch {
            if count == asset_ids.len() {
                return Ok(TrashRestoreResult {
                    restored_asset_ids: asset_ids
                        .iter()
                        .map(|id| id.as_str().to_string())
                        .collect(),
                });
            }
        }
        if asset_ids.len() == 1 {
            // The batch already was the individual request.
            return Ok(TrashRestoreResult {
                restored_asset_ids: Vec::new(),
            });
        }

        let mut restored_asset_ids = Vec::new();
        for asset_id in asset_ids {
            let outcome = self.post_restore(std::slice::from_ref(asset_id)).await?;
            if matches!(outcome, RestoreCall::Counted(1)) {
                restored_asset_ids.push(asset_id.as_str().to_string());
            }
        }
        Ok(TrashRestoreResult { restored_asset_ids })
    }

    /// Moves Remote Assets to the Immich trash (a soft delete: `force` is always `false`), preferring
    /// one batch request and falling back to one request per asset.
    ///
    /// Observed on Immich v3.4.1 (one server version only): `DELETE assets` with
    /// `{"ids": [...], "force": false}` answers 204 with an empty body when every ID is accessible
    /// (assets already in the trash are harmless). One unknown, unowned or locked ID fails the whole
    /// request with 400 and trashes nothing, and there is no per-asset result or count. So a batch
    /// is trusted only on 204; a 400 makes each asset be trashed on its own, where 204 means moved
    /// and 400 means that asset was not moved. A single asset is never requested twice. The
    /// permission is `asset.delete`.
    ///
    /// Failures abort the whole call with a credential-free code: `trash_move_authentication_required`
    /// (401), `trash_move_permission_denied:asset.delete` (403) and `trash_move_unavailable`
    /// otherwise (including transport errors and any success that is not 204); the response body is
    /// never forwarded. Moving is idempotent, so retrying after an abort is safe.
    pub async fn trash_assets(
        &self,
        asset_ids: &[RemoteAssetId],
    ) -> Result<TrashMoveResult, String> {
        if asset_ids.is_empty() {
            return Ok(TrashMoveResult {
                moved_asset_ids: Vec::new(),
            });
        }
        Self::validate_asset_ids(asset_ids, "Asset IDs for trashing must be unique")?;

        if matches!(self.delete_assets(asset_ids).await?, TrashCall::Moved) {
            return Ok(TrashMoveResult {
                moved_asset_ids: asset_ids.iter().map(|id| id.as_str().to_string()).collect(),
            });
        }
        if asset_ids.len() == 1 {
            // The batch already was the individual request.
            return Ok(TrashMoveResult {
                moved_asset_ids: Vec::new(),
            });
        }

        let mut moved_asset_ids = Vec::new();
        for asset_id in asset_ids {
            if matches!(
                self.delete_assets(std::slice::from_ref(asset_id)).await?,
                TrashCall::Moved
            ) {
                moved_asset_ids.push(asset_id.as_str().to_string());
            }
        }
        Ok(TrashMoveResult { moved_asset_ids })
    }

    /// Rejects IDs that are not path-safe and duplicate IDs (with `duplicate_message`) before any request,
    /// for the batch calls that Restore and Move to Trash share. The caller handles an empty list.
    fn validate_asset_ids(
        asset_ids: &[RemoteAssetId],
        duplicate_message: &str,
    ) -> Result<(), String> {
        for asset_id in asset_ids {
            Self::validate_path_segment(asset_id.as_str(), "asset ID")?;
        }
        let unique: HashSet<&str> = asset_ids.iter().map(|id| id.as_str()).collect();
        if unique.len() != asset_ids.len() {
            return Err(duplicate_message.to_string());
        }
        Ok(())
    }

    async fn delete_assets(&self, asset_ids: &[RemoteAssetId]) -> Result<TrashCall, String> {
        let response = self
            .request(Method::DELETE, "assets")
            // `force` is never `true`: Lymic never deletes permanently (ADR-0011).
            .json(&json!({ "ids": asset_ids, "force": false }))
            .send()
            .await
            .map_err(|_| "trash_move_unavailable".to_string())?;
        match response.status() {
            StatusCode::NO_CONTENT => Ok(TrashCall::Moved),
            StatusCode::BAD_REQUEST => Ok(TrashCall::Rejected),
            status => Err(request_failure_code("trash_move", status, "asset.delete")),
        }
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

    // Move to Trash (`DELETE assets`), fixtures from the live check on Immich v3.4.1 (#253).

    const NOT_FOUND_BODY: &str = r#"{"message":"Not found or no asset.delete access"}"#;

    #[tokio::test]
    async fn trashes_one_asset_with_a_soft_delete_request() {
        let (url, server) = serve_pages(vec![("204 No Content", "")]).await;

        let result = test_client(url, "test-key")
            .trash_assets(&ids(&["asset-1"]))
            .await
            .unwrap();

        assert_eq!(
            result,
            TrashMoveResult {
                moved_asset_ids: vec!["asset-1".into()],
            }
        );
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("DELETE /api/assets HTTP/1.1"));
        assert!(requests[0].contains("x-api-key: test-key"));
        assert!(requests[0].ends_with(r#"{"force":false,"ids":["asset-1"]}"#));
    }

    #[tokio::test]
    async fn trashes_all_requested_assets_with_one_batch_request() {
        let (url, server) = serve_pages(vec![("204 No Content", "")]).await;

        let result = test_client(url, "test-key")
            .trash_assets(&ids(&["asset-1", "asset-2", "asset-3"]))
            .await
            .unwrap();

        assert_eq!(
            result.moved_asset_ids,
            vec!["asset-1", "asset-2", "asset-3"]
        );
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with(r#"{"force":false,"ids":["asset-1","asset-2","asset-3"]}"#));
    }

    #[tokio::test]
    async fn trashes_each_asset_individually_when_the_batch_is_rejected() {
        let (url, server) = serve_pages(vec![
            ("400 Bad Request", NOT_FOUND_BODY),
            ("204 No Content", ""),
            ("400 Bad Request", NOT_FOUND_BODY),
            ("204 No Content", ""),
        ])
        .await;

        let result = test_client(url, "test-key")
            .trash_assets(&ids(&["asset-1", "gone-2", "asset-3"]))
            .await
            .unwrap();

        assert_eq!(result.moved_asset_ids, vec!["asset-1", "asset-3"]);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].ends_with(r#"{"force":false,"ids":["asset-1","gone-2","asset-3"]}"#));
        for (request, id) in requests[1..].iter().zip(["asset-1", "gone-2", "asset-3"]) {
            assert!(request.starts_with("DELETE /api/assets HTTP/1.1"));
            assert!(request.ends_with(&format!(r#"{{"force":false,"ids":["{id}"]}}"#)));
        }
    }

    #[tokio::test]
    async fn a_single_asset_rejected_by_the_trash_batch_is_not_requested_twice() {
        let (url, server) = serve_pages(vec![("400 Bad Request", NOT_FOUND_BODY)]).await;

        let result = test_client(url, "test-key")
            .trash_assets(&ids(&["gone-1"]))
            .await
            .unwrap();

        assert!(result.moved_asset_ids.is_empty());
        assert_eq!(server.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn never_sends_force_true_in_any_request_body() {
        let (url, server) = serve_pages(vec![
            ("400 Bad Request", NOT_FOUND_BODY),
            ("204 No Content", ""),
            ("204 No Content", ""),
        ])
        .await;

        test_client(url, "test-key")
            .trash_assets(&ids(&["asset-1", "asset-2"]))
            .await
            .unwrap();

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 3);
        for request in requests {
            let body = request.split("\r\n\r\n").nth(1).unwrap();
            let body: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["force"], serde_json::Value::Bool(false));
        }
    }

    #[tokio::test]
    async fn aborts_on_credential_or_server_failures_in_the_batch_call() {
        for (status, code) in [
            ("401 Unauthorized", "trash_move_authentication_required"),
            ("403 Forbidden", "trash_move_permission_denied:asset.delete"),
            ("500 Internal Server Error", "trash_move_unavailable"),
            ("502 Bad Gateway", "trash_move_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;

            let error = test_client(url, "trash-secret-key")
                .trash_assets(&ids(&["asset-1", "asset-2"]))
                .await
                .unwrap_err();

            assert_eq!(error, code);
            assert!(!error.contains("trash-secret-key"));
            assert!(!error.contains("stacktrace"));
            assert_eq!(server.await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn aborts_the_trash_fallback_on_credential_or_server_failures() {
        for (status, code) in [
            ("401 Unauthorized", "trash_move_authentication_required"),
            ("403 Forbidden", "trash_move_permission_denied:asset.delete"),
            ("500 Internal Server Error", "trash_move_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![
                ("400 Bad Request", NOT_FOUND_BODY),
                ("204 No Content", ""),
                (status, "internal secret stacktrace"),
            ])
            .await;

            let error = test_client(url, "trash-secret-key")
                .trash_assets(&ids(&["asset-1", "asset-2", "asset-3"]))
                .await
                .unwrap_err();

            assert_eq!(error, code);
            assert_eq!(server.await.unwrap().len(), 3);
        }
    }

    #[tokio::test]
    async fn aborts_the_trash_fallback_on_a_success_that_is_not_an_empty_204() {
        let (url, server) = serve_pages(vec![
            ("400 Bad Request", NOT_FOUND_BODY),
            ("204 No Content", ""),
            ("200 OK", r#"{"count":1}"#),
        ])
        .await;

        let error = test_client(url, "test-key")
            .trash_assets(&ids(&["asset-1", "asset-2", "asset-3"]))
            .await
            .unwrap_err();

        assert_eq!(error, "trash_move_unavailable");
        assert_eq!(server.await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn reports_no_moved_asset_when_the_batch_and_every_individual_request_are_rejected() {
        let (url, server) = serve_pages(vec![
            ("400 Bad Request", NOT_FOUND_BODY),
            ("400 Bad Request", NOT_FOUND_BODY),
            ("400 Bad Request", NOT_FOUND_BODY),
        ])
        .await;

        let result = test_client(url, "test-key")
            .trash_assets(&ids(&["gone-1", "gone-2"]))
            .await
            .unwrap();

        assert!(result.moved_asset_ids.is_empty());
        assert_eq!(server.await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn aborts_on_a_transport_error() {
        let error = test_client("http://127.0.0.1:9".to_string(), "trash-secret-key")
            .trash_assets(&ids(&["asset-1"]))
            .await
            .unwrap_err();

        assert_eq!(error, "trash_move_unavailable");
    }

    #[tokio::test]
    async fn does_not_trust_a_success_that_is_not_an_empty_204() {
        for (status, body) in [
            ("200 OK", r#"{"count":1}"#),
            ("200 OK", "{not-json"),
            ("202 Accepted", ""),
        ] {
            let (url, server) = serve_pages(vec![(status, body)]).await;

            let error = test_client(url, "trash-secret-key")
                .trash_assets(&ids(&["asset-1"]))
                .await
                .unwrap_err();

            assert_eq!(error, "trash_move_unavailable");
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_unsafe_duplicate_or_empty_ids_before_requesting() {
        let client = test_client("http://127.0.0.1:9".to_string(), "test-key");
        assert_eq!(
            client.trash_assets(&ids(&["a/b"])).await.unwrap_err(),
            "Invalid asset ID for API path"
        );
        assert_eq!(
            client.trash_assets(&ids(&["a", "a"])).await.unwrap_err(),
            "Asset IDs for trashing must be unique"
        );
        assert_eq!(
            client.trash_assets(&[]).await.unwrap(),
            TrashMoveResult {
                moved_asset_ids: vec![]
            }
        );
    }
}
