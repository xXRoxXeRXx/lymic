use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, request_failure_code, MAX_SUCCESS_RESPONSE_BYTES,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};

/// A Tag of the Immich Library. Immich returns Tags as a flat list; the hierarchy is `parent_id`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Tag {
    pub id: String,
    /// The Tag's own name, without its ancestors.
    pub name: String,
    /// The Tag's full path, for example `Travel/Italy/Rome`.
    pub value: String,
    pub parent_id: Option<String>,
    /// A `#RRGGBB` colour as Immich returns it; the frontend decides whether it is usable.
    pub color: Option<String>,
}

/// One Tag of `GET /api/tags`. Only the fields Lymic shows are read; Immich may add others.
#[derive(Deserialize)]
struct TagResponse {
    id: String,
    name: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(rename = "parentId", default)]
    parent_id: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

impl From<TagResponse> for Tag {
    fn from(response: TagResponse) -> Self {
        Self {
            // Without a full path the name is the best label there is.
            value: non_empty(response.value).unwrap_or_else(|| response.name.clone()),
            id: response.id,
            name: response.name,
            parent_id: non_empty(response.parent_id),
            color: non_empty(response.color),
        }
    }
}

impl ImmichClient {
    /// Lists every Tag of the Immich Library. Errors carry a stable `tags_*` code and never the
    /// response body.
    pub async fn get_tags(&self) -> Result<Vec<Tag>, String> {
        let response = self
            .request(Method::GET, "tags")
            .send()
            .await
            .map_err(|_| "tags_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(request_failure_code("tags", response.status(), "tag.read"));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let tags: Vec<TagResponse> =
            serde_json::from_slice(&body).map_err(|_| "tags_invalid_response".to_string())?;
        Ok(tags.into_iter().map(Tag::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Tag;
    use crate::immich::test_support::{serve_once, test_client};

    async fn request_with_response(status: &str, body: &str) -> (Result<Vec<Tag>, String>, String) {
        let (url, server) = serve_once(status, body).await;
        let result = test_client(url, "test-key").get_tags().await;
        (result, server.await.unwrap())
    }

    #[tokio::test]
    async fn requests_the_tag_list_with_the_api_key_and_maps_every_field() {
        let body = r##"[{"id":"t1","name":"Rome","value":"Travel/Italy/Rome","parentId":"t0","color":"#ff0000","createdAt":"2025-01-01T00:00:00.000Z","updatedAt":"2025-01-02T00:00:00.000Z"},{"id":"t0","name":"Italy","value":"Travel/Italy","parentId":null,"color":null}]"##;
        let (tags, request) = request_with_response("200 OK", body).await;
        assert!(request.starts_with("GET /api/tags HTTP/1.1"));
        assert!(request.contains("x-api-key: test-key"));
        assert_eq!(
            tags.unwrap(),
            vec![
                Tag {
                    id: "t1".into(),
                    name: "Rome".into(),
                    value: "Travel/Italy/Rome".into(),
                    parent_id: Some("t0".into()),
                    color: Some("#ff0000".into()),
                },
                Tag {
                    id: "t0".into(),
                    name: "Italy".into(),
                    value: "Travel/Italy".into(),
                    parent_id: None,
                    color: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn tolerates_absent_optional_fields_and_unknown_fields() {
        let body = r#"[{"id":"t1","name":"Travel","value":"Travel","extra":1},{"id":"t2","name":"Rome","parentId":"","color":""}]"#;
        let (tags, _) = request_with_response("200 OK", body).await;
        let tags = tags.unwrap();
        assert_eq!(tags[0].parent_id, None);
        assert_eq!(tags[0].color, None);
        // An absent full path falls back to the name; empty strings count as absent.
        assert_eq!(tags[1].value, "Rome");
        assert_eq!(tags[1].parent_id, None);
        assert_eq!(tags[1].color, None);
    }

    #[tokio::test]
    async fn serializes_tags_for_the_frontend_in_camel_case() {
        let tag = Tag {
            id: "t1".into(),
            name: "Rome".into(),
            value: "Italy/Rome".into(),
            parent_id: Some("t0".into()),
            color: None,
        };
        assert_eq!(
            serde_json::to_string(&tag).unwrap(),
            r#"{"id":"t1","name":"Rome","value":"Italy/Rome","parentId":"t0","color":null}"#
        );
    }

    #[tokio::test]
    async fn returns_no_tags_for_an_empty_response() {
        let (tags, _) = request_with_response("200 OK", "[]").await;
        assert_eq!(tags.unwrap(), vec![]);
    }

    #[tokio::test]
    async fn maps_401_to_authentication_required() {
        let (result, _) = request_with_response("401 Unauthorized", "secret").await;
        assert_eq!(result.unwrap_err(), "tags_authentication_required");
    }

    #[tokio::test]
    async fn maps_403_to_permission_denied_naming_tag_read_without_the_body() {
        let (result, _) = request_with_response("403 Forbidden", "secret").await;
        let error = result.unwrap_err();
        assert_eq!(error, "tags_permission_denied:tag.read");
        assert!(!error.contains("secret"));
    }

    #[tokio::test]
    async fn maps_other_failures_and_malformed_bodies_without_leaking_the_body() {
        let (server_error, _) = request_with_response("500 Internal Server Error", "secret").await;
        assert_eq!(server_error.unwrap_err(), "tags_unavailable");
        let (malformed, _) = request_with_response("200 OK", "{secret").await;
        assert_eq!(malformed.unwrap_err(), "tags_invalid_response");
        let (wrong_shape, _) = request_with_response("200 OK", r#"[{"id":"t1"}]"#).await;
        assert_eq!(wrong_shape.unwrap_err(), "tags_invalid_response");
    }

    #[tokio::test]
    async fn reports_an_unreachable_server_as_unavailable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let result = test_client(url, "test-key").get_tags().await;
        assert_eq!(result.unwrap_err(), "tags_unavailable");
    }
}
