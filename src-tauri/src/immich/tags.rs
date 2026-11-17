use super::client::ImmichClient;
use super::response::{
    non_empty, read_response_body_limited, request_failure_code, MAX_SUCCESS_RESPONSE_BYTES,
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

/// A Tag of one Remote Asset as shown in the Lightbox. Only the fields a chip needs; the
/// hierarchy is not needed there.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssetTag {
    pub id: String,
    /// The Tag's own name, without its ancestors.
    pub name: String,
    /// The Tag's full path, for example `Travel/Italy/Rome`.
    pub value: String,
    /// A `#RRGGBB` colour as Immich returns it; the frontend decides whether it is usable.
    pub color: Option<String>,
}

/// One Tag as Immich sends it, in `GET /api/tags` and in the `tags` of an asset's details. Only
/// the fields Lymic shows are read; Immich may add others. Every field is optional here so each
/// caller decides what a usable entry needs.
#[derive(Default, Deserialize)]
struct TagEntry {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(rename = "parentId", default)]
    parent_id: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

impl TagEntry {
    /// An entry of the Tag list needs its id and its name.
    fn into_tag(self) -> Option<Tag> {
        let name = self.name?;
        Some(Tag {
            // Without a full path the name is the best label there is.
            value: non_empty(self.value).unwrap_or_else(|| name.clone()),
            id: self.id?,
            name,
            parent_id: non_empty(self.parent_id),
            color: non_empty(self.color),
        })
    }

    /// A Tag can only be opened by its id; the name falls back to the last segment of the path
    /// and the path to the name. An entry with neither is dropped.
    fn into_asset_tag(self) -> Option<AssetTag> {
        let id = non_empty(self.id)?;
        let (name, value) = match (non_empty(self.name), non_empty(self.value)) {
            (Some(name), Some(value)) => (name, value),
            (Some(name), None) => (name.clone(), name),
            (None, Some(value)) => (
                value.rsplit('/').next().unwrap_or(&value).to_string(),
                value,
            ),
            (None, None) => return None,
        };
        Some(AssetTag {
            id,
            name,
            value,
            color: non_empty(self.color),
        })
    }
}

/// The Tags of an asset details response, kept raw until here so one malformed entry (or a
/// `tags` that is not a list) never fails the whole details request. `None` when the server does
/// not report Tags.
pub(super) fn asset_tags_from_value(value: serde_json::Value) -> Option<Vec<AssetTag>> {
    match value {
        serde_json::Value::Array(entries) => Some(
            entries
                .into_iter()
                .filter_map(|entry| serde_json::from_value::<TagEntry>(entry).ok())
                .filter_map(TagEntry::into_asset_tag)
                .collect(),
        ),
        _ => None,
    }
}

impl ImmichClient {
    /// Lists every Tag of the Immich Library. Errors carry a stable `tags_*` code and never the
    /// response body, also when the body cannot be read in full (too large or cut off).
    pub async fn get_tags(&self) -> Result<Vec<Tag>, String> {
        let response = self
            .request(Method::GET, "tags")
            .send()
            .await
            .map_err(|_| "tags_unavailable".to_string())?;
        if !response.status().is_success() {
            return Err(request_failure_code("tags", response.status(), "tag.read"));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES)
            .await
            .map_err(|_| "tags_invalid_response".to_string())?;
        let entries: Vec<TagEntry> =
            serde_json::from_slice(&body).map_err(|_| "tags_invalid_response".to_string())?;
        entries
            .into_iter()
            .map(TagEntry::into_tag)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "tags_invalid_response".to_string())
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
    async fn treats_whitespace_only_fields_as_absent_for_tags_and_asset_tags() {
        let blank = r#"{"id":"t1","name":"Rome","value":"  ","parentId":" ","color":"\u0009"}"#;
        let (tags, _) = request_with_response("200 OK", &format!("[{blank}]")).await;
        let tag = tags.unwrap().remove(0);
        assert_eq!(tag.value, "Rome");
        assert_eq!(tag.parent_id, None);
        assert_eq!(tag.color, None);

        let asset_tags =
            super::asset_tags_from_value(serde_json::from_str(&format!("[{blank}]")).unwrap())
                .unwrap();
        assert_eq!(asset_tags.len(), 1);
        assert_eq!(asset_tags[0].value, "Rome");
        assert_eq!(asset_tags[0].color, None);
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
    async fn maps_an_oversized_body_to_an_invalid_response_without_leaking_it() {
        let body = format!(r#"["{}"]"#, "secret".repeat(200 * 1024));
        let (result, _) = request_with_response("200 OK", &body).await;
        let error = result.unwrap_err();
        assert_eq!(error, "tags_invalid_response");
        assert!(!error.contains("secret"));
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
