use super::client::ImmichClient;
use super::response::{
    explore_status_error, read_response_body_limited, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};

/// A city in which the user has Remote Assets, with the Remote Asset Immich picked for it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Place {
    pub city: String,
    pub thumbnail_asset_id: String,
}

/// One asset of `GET /api/search/cities`; Immich returns one asset per city.
#[derive(Deserialize)]
struct CityAssetResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "exifInfo", default)]
    exif: Option<CityExif>,
}

#[derive(Deserialize)]
struct CityExif {
    #[serde(default)]
    city: Option<String>,
}

impl CityAssetResponse {
    fn into_place(self) -> Option<Place> {
        let city = self.exif?.city?;
        let id = self.id.filter(|id| !id.is_empty())?;
        // Immich's city filter matches exactly, so the name is kept as returned.
        (!city.trim().is_empty()).then_some(Place {
            city,
            thumbnail_asset_id: id,
        })
    }
}

impl ImmichClient {
    /// Lists the Places (cities) of the authenticated user's library. Errors carry a
    /// stable `places_*` code and never the response body.
    pub async fn get_places(&self) -> Result<Vec<Place>, String> {
        let response = self
            .request(Method::GET, "search/cities")
            .send()
            .await
            .map_err(|_| "places_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(explore_status_error(
                "places",
                response.status(),
                "configure an API key with asset.read permission",
            ));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let cities: Vec<CityAssetResponse> =
            serde_json::from_slice(&body).map_err(|_| "places_invalid_response".to_string())?;
        Ok(cities
            .into_iter()
            .filter_map(CityAssetResponse::into_place)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Place;
    use crate::immich::test_support::{serve_once, test_client};

    /// Serves one canned response and returns the request line plus headers it received.
    async fn request_with_response(
        status: &str,
        body: &str,
    ) -> (Result<Vec<Place>, String>, String) {
        let (url, server) = serve_once(status, body).await;
        let result = test_client(url, "test-key").get_places().await;
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn requests_cities_with_the_api_key_and_returns_one_place_per_city() {
        let body = r#"[{"id":"asset-1","exifInfo":{"city":"Paris","country":"France"}},{"id":"asset-2","exifInfo":{"city":"Berlin"}}]"#;
        let (places, request) = request_with_response("200 OK", body).await;
        assert!(request.starts_with("GET /api/search/cities HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert_eq!(
            places.unwrap(),
            vec![
                Place {
                    city: "Paris".into(),
                    thumbnail_asset_id: "asset-1".into()
                },
                Place {
                    city: "Berlin".into(),
                    thumbnail_asset_id: "asset-2".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn omits_entries_without_a_city_or_asset_id_and_ignores_missing_optional_fields() {
        let body = r#"[{"id":"a","exifInfo":{"city":"Oslo"}},{"id":"b"},{"id":"c","exifInfo":{}},{"id":"d","exifInfo":{"city":null}},{"id":"e","exifInfo":{"city":"  "}},{"exifInfo":{"city":"Rome"}},{"id":"f","exifInfo":{"city":"Lima"},"unknown":1},{"id":"g","exifInfo":{"city":" Rio "}}]"#;
        let (places, _) = request_with_response("200 OK", body).await;
        let cities: Vec<_> = places
            .unwrap()
            .into_iter()
            .map(|place| place.city)
            .collect();
        // The name is kept exactly as Immich returned it: its city filter matches exactly.
        assert_eq!(cities, vec!["Oslo", "Lima", " Rio "]);
    }

    #[tokio::test]
    async fn serializes_places_for_the_frontend_in_camel_case() {
        let place = Place {
            city: "Paris".into(),
            thumbnail_asset_id: "asset-1".into(),
        };
        assert_eq!(
            serde_json::to_string(&place).unwrap(),
            r#"{"city":"Paris","thumbnailAssetId":"asset-1"}"#
        );
    }

    #[tokio::test]
    async fn returns_no_places_for_an_empty_response() {
        let (places, _) = request_with_response("200 OK", "[]").await;
        assert_eq!(places.unwrap(), vec![]);
    }

    #[tokio::test]
    async fn maps_401_to_authentication_required() {
        let (result, _) = request_with_response("401 Unauthorized", "secret").await;
        assert_eq!(result.unwrap_err(), "places_authentication_required");
    }

    #[tokio::test]
    async fn maps_403_to_permission_denied_naming_asset_read_without_the_body() {
        let (result, _) = request_with_response("403 Forbidden", "secret").await;
        let error = result.unwrap_err();
        assert!(error.starts_with("places_permission_denied:"));
        assert!(error.contains("asset.read"));
        assert!(!error.contains("secret"));
    }

    #[tokio::test]
    async fn maps_other_failures_and_malformed_bodies_without_leaking_the_body() {
        let (server_error, _) = request_with_response("500 Internal Server Error", "secret").await;
        assert_eq!(server_error.unwrap_err(), "places_unavailable");
        let (malformed, _) = request_with_response("200 OK", "{secret").await;
        let error = malformed.unwrap_err();
        assert_eq!(error, "places_invalid_response");
    }
}
