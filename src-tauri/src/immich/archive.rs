//! Archive and Unarchive of Remote Assets: the only Immich write that changes an asset's visibility.

use super::client::ImmichClient;
use super::ids::RemoteAssetId;
use super::response::request_failure_code;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The only visibilities Lymic writes. `locked` and `hidden` cannot be expressed, so they can never be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AssetVisibility {
    /// Archived: hidden from the Photo and Favorites Timelines, shown in the Archive Timeline.
    Archive,
    /// Not archived: shown in the Photo Timeline.
    Timeline,
}

/// The requested Remote Assets Immich confirmed as changed; every other requested asset kept its visibility.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssetVisibilityResult {
    pub changed_asset_ids: Vec<String>,
}

/// What one visibility update call told us.
enum VisibilityCall {
    /// A 2xx: Immich applied the update.
    Applied,
    /// A 400: Immich refuses the whole request, for instance because one ID is unknown or not the user's.
    Rejected,
}

impl ImmichClient {
    /// Sets the visibility of Remote Assets, preferring one bulk request and falling back to one request per asset.
    ///
    /// Bulk `PUT assets` with `{"ids": [...], "visibility": ...}` answers 204 without a per-asset
    /// result; the permission is `asset.update`. Observed on Immich v3.4.1 (one server version
    /// only, see the Application Map): one unknown or unowned ID fails the whole request with 400 and
    /// changes nothing, and an asset already in the target state is harmless. A 2xx bulk answer is
    /// therefore trusted for the whole selection, and a 400 makes each asset be updated on its own with
    /// the same verified `PUT assets` endpoint and a single-element `ids` array (2xx means changed, 400
    /// means that asset failed), so a stale ID cannot block the others. A single asset is never requested
    /// twice. The server also accepts `hidden` and `locked`; only `AssetVisibility` keeps Lymic from sending them.
    ///
    /// Failures abort the whole call with a credential-free code: `archive_authentication_required`
    /// (401), `archive_permission_denied:asset.update` (403) and `archive_unavailable` otherwise
    /// (including transport errors); the response body is never read or forwarded.
    pub async fn set_assets_visibility(
        &self,
        asset_ids: &[RemoteAssetId],
        visibility: AssetVisibility,
    ) -> Result<AssetVisibilityResult, String> {
        let changed_asset_ids = Self::run_bulk_with_per_asset_fallback(
            asset_ids,
            "Asset IDs for visibility must be unique",
            |ids| async move {
                Ok(matches!(
                    self.put_visibility(ids, visibility).await?,
                    VisibilityCall::Applied
                ))
            },
            |asset_id| async move {
                Ok(matches!(
                    self.put_visibility(std::slice::from_ref(asset_id), visibility)
                        .await?,
                    VisibilityCall::Applied
                ))
            },
        )
        .await?;
        Ok(AssetVisibilityResult { changed_asset_ids })
    }

    async fn put_visibility(
        &self,
        asset_ids: &[RemoteAssetId],
        visibility: AssetVisibility,
    ) -> Result<VisibilityCall, String> {
        let response = self
            .request(Method::PUT, "assets")
            .json(&json!({ "ids": asset_ids, "visibility": visibility }))
            .send()
            .await
            .map_err(|_| "archive_unavailable".to_string())?;
        let status = response.status();
        if status == StatusCode::BAD_REQUEST {
            return Ok(VisibilityCall::Rejected);
        }
        if !status.is_success() {
            return Err(request_failure_code("archive", status, "asset.update"));
        }
        Ok(VisibilityCall::Applied)
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

    #[test]
    fn accepts_only_the_archive_and_timeline_visibilities() {
        for (text, expected) in [
            (r#""archive""#, AssetVisibility::Archive),
            (r#""timeline""#, AssetVisibility::Timeline),
        ] {
            assert_eq!(
                serde_json::from_str::<AssetVisibility>(text).unwrap(),
                expected
            );
            assert_eq!(serde_json::to_string(&expected).unwrap(), text);
        }
        for text in [
            r#""locked""#,
            r#""hidden""#,
            r#""Archive""#,
            r#""""#,
            "null",
            "1",
        ] {
            assert!(
                serde_json::from_str::<AssetVisibility>(text).is_err(),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn changes_all_requested_assets_with_one_bulk_request() {
        for (visibility, wire) in [
            (AssetVisibility::Timeline, "timeline"),
            (AssetVisibility::Archive, "archive"),
        ] {
            let (url, server) = serve_pages(vec![("204 No Content", "")]).await;

            let result = test_client(url, "test-key")
                .set_assets_visibility(&ids(&["asset-1", "asset-2"]), visibility)
                .await
                .unwrap();

            assert_eq!(
                result,
                AssetVisibilityResult {
                    changed_asset_ids: vec!["asset-1".into(), "asset-2".into()],
                }
            );
            let requests = server.await.unwrap();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].starts_with("PUT /api/assets HTTP/1.1"));
            assert!(requests[0].contains("x-api-key: test-key"));
            assert!(requests[0].ends_with(&format!(
                r#"{{"ids":["asset-1","asset-2"],"visibility":"{wire}"}}"#
            )));
        }
    }

    #[tokio::test]
    async fn changes_each_asset_individually_when_the_batch_is_rejected() {
        let (url, server) = serve_pages(vec![
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.update access"}"#,
            ),
            ("200 OK", r#"{"id":"asset-1"}"#),
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.update access"}"#,
            ),
            ("200 OK", r#"{"id":"asset-3"}"#),
        ])
        .await;

        let result = test_client(url, "test-key")
            .set_assets_visibility(
                &ids(&["asset-1", "gone-2", "asset-3"]),
                AssetVisibility::Timeline,
            )
            .await
            .unwrap();

        assert_eq!(result.changed_asset_ids, vec!["asset-1", "asset-3"]);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].starts_with("PUT /api/assets HTTP/1.1"));
        assert!(requests[0]
            .ends_with(r#"{"ids":["asset-1","gone-2","asset-3"],"visibility":"timeline"}"#));
        for (request, id) in requests[1..].iter().zip(["asset-1", "gone-2", "asset-3"]) {
            assert!(request.starts_with("PUT /api/assets HTTP/1.1"));
            assert!(request.ends_with(&format!(r#"{{"ids":["{id}"],"visibility":"timeline"}}"#)));
        }
    }

    #[tokio::test]
    async fn archives_each_asset_individually_when_the_batch_is_rejected() {
        let (url, server) = serve_pages(vec![
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.update access"}"#,
            ),
            ("204 No Content", ""),
            (
                "400 Bad Request",
                r#"{"message":"Not found or no asset.update access"}"#,
            ),
        ])
        .await;

        let result = test_client(url, "test-key")
            .set_assets_visibility(&ids(&["asset-1", "gone-2"]), AssetVisibility::Archive)
            .await
            .unwrap();

        assert_eq!(result.changed_asset_ids, vec!["asset-1"]);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].ends_with(r#"{"ids":["asset-1","gone-2"],"visibility":"archive"}"#));
        for (request, id) in requests[1..].iter().zip(["asset-1", "gone-2"]) {
            assert!(request.starts_with("PUT /api/assets HTTP/1.1"));
            assert!(request.ends_with(&format!(r#"{{"ids":["{id}"],"visibility":"archive"}}"#)));
        }
    }

    #[tokio::test]
    async fn a_single_asset_rejected_by_the_batch_call_is_not_requested_twice() {
        let (url, server) =
            serve_pages(vec![("400 Bad Request", r#"{"message":"Not found"}"#)]).await;

        let result = test_client(url, "test-key")
            .set_assets_visibility(&ids(&["gone-1"]), AssetVisibility::Archive)
            .await
            .unwrap();

        assert!(result.changed_asset_ids.is_empty());
        assert_eq!(server.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_single_asset_is_changed_with_one_request() {
        let (url, server) = serve_pages(vec![("204 No Content", "")]).await;

        let result = test_client(url, "test-key")
            .set_assets_visibility(&ids(&["asset-1"]), AssetVisibility::Archive)
            .await
            .unwrap();

        assert_eq!(result.changed_asset_ids, vec!["asset-1"]);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("PUT /api/assets HTTP/1.1"));
        assert!(requests[0].ends_with(r#"{"ids":["asset-1"],"visibility":"archive"}"#));
    }

    #[tokio::test]
    async fn aborts_the_fallback_on_credential_or_server_failures() {
        for (status, code) in [
            ("401 Unauthorized", "archive_authentication_required"),
            ("403 Forbidden", "archive_permission_denied:asset.update"),
            ("500 Internal Server Error", "archive_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![
                ("400 Bad Request", "{}"),
                ("200 OK", "{}"),
                (status, "internal secret stacktrace"),
            ])
            .await;

            let error = test_client(url, "archive-secret-key")
                .set_assets_visibility(&ids(&["a-1", "a-2", "a-3"]), AssetVisibility::Archive)
                .await
                .unwrap_err();

            assert_eq!(error, code);
            assert_eq!(server.await.unwrap().len(), 3);
        }
    }

    #[tokio::test]
    async fn classifies_failures_without_leaking_the_body_or_the_api_key() {
        for (status, code) in [
            ("401 Unauthorized", "archive_authentication_required"),
            ("403 Forbidden", "archive_permission_denied:asset.update"),
            ("500 Internal Server Error", "archive_unavailable"),
            ("502 Bad Gateway", "archive_unavailable"),
            ("404 Not Found", "archive_unavailable"),
        ] {
            let (url, server) = serve_pages(vec![(status, "internal secret stacktrace")]).await;
            let error = test_client(url, "archive-secret-key")
                .set_assets_visibility(&ids(&["a-1", "a-2"]), AssetVisibility::Timeline)
                .await
                .unwrap_err();
            server.await.unwrap();
            assert_eq!(error, code);
            assert!(!error.contains("secret"));
        }
    }

    #[tokio::test]
    async fn reports_a_transport_error_as_unavailable() {
        let error = test_client("http://127.0.0.1:9".to_string(), "test-key")
            .set_assets_visibility(&ids(&["a-1", "a-2"]), AssetVisibility::Timeline)
            .await
            .unwrap_err();
        assert_eq!(error, "archive_unavailable");
    }

    #[tokio::test]
    async fn rejects_unsafe_or_duplicate_ids_before_requesting() {
        let client = test_client("http://127.0.0.1:9".to_string(), "test-key");
        assert_eq!(
            client
                .set_assets_visibility(&ids(&["a/b"]), AssetVisibility::Archive)
                .await
                .unwrap_err(),
            "Invalid asset ID for API path"
        );
        assert_eq!(
            client
                .set_assets_visibility(&ids(&["a", "a"]), AssetVisibility::Archive)
                .await
                .unwrap_err(),
            "Asset IDs for visibility must be unique"
        );
        assert_eq!(
            client
                .set_assets_visibility(&[], AssetVisibility::Archive)
                .await
                .unwrap(),
            AssetVisibilityResult {
                changed_asset_ids: vec![]
            }
        );
    }
}
