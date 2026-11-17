//! Suggestions for Search Filters whose values Immich compares exactly (`GET
//! /api/search/suggestions`). The request is generic: a suggestion type plus optional
//! narrowing values. Camera make, model, lens model and the Location country and state use it;
//! a new suggestion type adds a variant without changing this module's shape.

use super::client::ImmichClient;
use super::search::{send_json, SUGGESTIONS_MAX_BYTES};
use reqwest::Method;
use serde::Deserialize;

/// What Immich suggests values for.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SuggestionType {
    Country,
    State,
    CameraMake,
    CameraModel,
    CameraLensModel,
}

impl SuggestionType {
    /// The `type` query value Immich expects.
    fn immich_name(self) -> &'static str {
        match self {
            SuggestionType::Country => "country",
            SuggestionType::State => "state",
            SuggestionType::CameraMake => "camera-make",
            SuggestionType::CameraModel => "camera-model",
            SuggestionType::CameraLensModel => "camera-lens-model",
        }
    }
}

/// One suggestion request as the frontend describes it. The narrowing values limit the
/// suggestions to those that occur together with them (for example the models of one make).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuggestionRequest {
    #[serde(rename = "type")]
    pub suggestion_type: SuggestionType,
    #[serde(default)]
    pub make: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

impl SuggestionRequest {
    fn query(&self) -> Vec<(&'static str, &str)> {
        let mut query = vec![("type", self.suggestion_type.immich_name())];
        for (key, value) in [
            ("make", &self.make),
            ("country", &self.country),
            ("state", &self.state),
        ] {
            if let Some(value) = value.as_deref().filter(|value| !value.trim().is_empty()) {
                query.push((key, value));
            }
        }
        query
    }
}

impl ImmichClient {
    /// Lists the values Immich suggests for a Search Filter, in the order Immich returns
    /// them. Blank values are dropped. Errors carry a stable `search_*` code and never the
    /// response body.
    pub async fn get_search_suggestions(
        &self,
        request: &SuggestionRequest,
    ) -> Result<Vec<String>, String> {
        let suggestions: Vec<Option<String>> = send_json(
            self.request(Method::GET, "search/suggestions")
                .query(&request.query()),
            SUGGESTIONS_MAX_BYTES,
        )
        .await?;
        Ok(suggestions
            .into_iter()
            .flatten()
            .filter(|suggestion| !suggestion.trim().is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{SuggestionRequest, SuggestionType};
    use crate::immich::test_support::{serve_once, test_client};

    fn request(suggestion_type: SuggestionType) -> SuggestionRequest {
        SuggestionRequest {
            suggestion_type,
            make: None,
            country: None,
            state: None,
        }
    }

    /// Serves one canned response; returns the result and the request line plus headers.
    async fn suggest(
        status: &str,
        body: &str,
        request: SuggestionRequest,
    ) -> (Result<Vec<String>, String>, String) {
        let (url, server) = serve_once(status, body).await;
        let result = test_client(url, "test-key")
            .get_search_suggestions(&request)
            .await;
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn suggestions_are_read_with_get_and_the_type_and_returned_in_order() {
        let (result, sent) = suggest(
            "200 OK",
            r#"["Canon","FUJIFILM","Sony"]"#,
            request(SuggestionType::CameraMake),
        )
        .await;

        assert!(sent.starts_with("GET /api/search/suggestions?type=camera-make HTTP/1.1"));
        assert!(sent.contains("x-api-key: test-key"));
        assert_eq!(result.unwrap(), vec!["Canon", "FUJIFILM", "Sony"]);
    }

    #[tokio::test]
    async fn every_suggestion_type_is_sent_under_the_name_immich_expects() {
        for (suggestion_type, name) in [
            (SuggestionType::Country, "country"),
            (SuggestionType::State, "state"),
            (SuggestionType::CameraMake, "camera-make"),
            (SuggestionType::CameraModel, "camera-model"),
            (SuggestionType::CameraLensModel, "camera-lens-model"),
        ] {
            let (_, sent) = suggest("200 OK", "[]", request(suggestion_type)).await;
            assert!(
                sent.starts_with(&format!("GET /api/search/suggestions?type={name} HTTP/1.1")),
                "{name}: {sent}"
            );
        }
    }

    #[tokio::test]
    async fn model_suggestions_are_narrowed_by_the_make_and_the_value_is_encoded() {
        let narrowed = SuggestionRequest {
            make: Some("Leica & Co".to_string()),
            ..request(SuggestionType::CameraModel)
        };
        let (_, sent) = suggest("200 OK", "[]", narrowed).await;

        assert!(
            sent.starts_with(
                "GET /api/search/suggestions?type=camera-model&make=Leica+%26+Co HTTP/1.1"
            ),
            "{sent}"
        );
    }

    #[tokio::test]
    async fn country_and_state_narrow_too_and_blank_narrowing_is_left_out() {
        let narrowed = SuggestionRequest {
            country: Some("Italy".to_string()),
            state: Some("Tuscany".to_string()),
            make: Some("  ".to_string()),
            ..request(SuggestionType::State)
        };
        let (_, sent) = suggest("200 OK", "[]", narrowed).await;

        assert!(
            sent.starts_with(
                "GET /api/search/suggestions?type=state&country=Italy&state=Tuscany HTTP/1.1"
            ),
            "{sent}"
        );
    }

    #[tokio::test]
    async fn blank_and_null_suggestions_are_dropped() {
        let (result, _) = suggest(
            "200 OK",
            r#"["Canon", null, "", "  ", "Nikon"]"#,
            request(SuggestionType::CameraMake),
        )
        .await;

        assert_eq!(result.unwrap(), vec!["Canon", "Nikon"]);
    }

    #[tokio::test]
    async fn a_failed_suggestion_request_returns_a_search_code_without_the_response_body() {
        for (status, expected) in [
            ("401 Unauthorized", "search_authentication_required"),
            ("403 Forbidden", "search_permission_denied:asset.read"),
            ("500 Internal Server Error", "search_unavailable"),
        ] {
            let (result, _) =
                suggest(status, "secret body", request(SuggestionType::CameraMake)).await;
            let error = result.unwrap_err();
            assert_eq!(error, expected, "{status}");
            assert!(!error.contains("secret"));
        }
    }

    #[tokio::test]
    async fn a_malformed_suggestion_response_is_an_invalid_response() {
        for body in ["not json", r#"{"suggestions":[]}"#, "[1, 2]"] {
            let (result, _) = suggest("200 OK", body, request(SuggestionType::CameraMake)).await;
            assert_eq!(result.unwrap_err(), "search_invalid_response", "{body}");
        }
    }

    #[test]
    fn a_request_deserializes_from_the_frontend_shape() {
        let parsed: SuggestionRequest =
            serde_json::from_str(r#"{"type":"cameraLensModel","make":"Canon"}"#).unwrap();
        assert_eq!(
            parsed,
            SuggestionRequest {
                make: Some("Canon".to_string()),
                ..request(SuggestionType::CameraLensModel)
            }
        );
    }
}
