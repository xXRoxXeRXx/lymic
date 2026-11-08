use super::client::ImmichClient;
use super::response::{read_response_body_limited, MAX_SUCCESS_RESPONSE_BYTES};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

fn recognized_people_access_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "recognized_people_authentication_required:people".to_string(),
        StatusCode::FORBIDDEN => "recognized_people_permission_denied:people:configure an API key with person.read and asset.read permissions".to_string(),
        _ => "recognized_people_unavailable:people".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecognizedPerson {
    pub id: String,
    pub name: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeoplePage {
    pub people: Vec<RecognizedPerson>,
    pub has_next_page: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeopleResponse {
    #[serde(default)]
    people: Vec<RecognizedPerson>,
    total: Option<u64>,
    #[serde(default)]
    has_next_page: Option<bool>,
}

impl ImmichClient {
    pub async fn get_people(&self, page: u32, size: u32) -> Result<PeoplePage, String> {
        let response = self
            .client
            .get(self.endpoint_url("people"))
            .query(&[("page", page), ("size", size)])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|_| "Unable to load recognized people".to_string())?;
        if !response.status().is_success() {
            return Err(recognized_people_access_error(response.status()));
        }
        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let response: PeopleResponse = serde_json::from_slice(&body)
            .map_err(|_| "Invalid recognized people response".to_string())?;
        Ok(PeoplePage {
            has_next_page: response.has_next_page.unwrap_or_else(|| {
                response
                    .total
                    .is_some_and(|total| u64::from(page) * u64::from(size) < total)
            }),
            people: response.people,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PeopleResponse;

    #[test]
    fn accepts_immich_people_without_asset_count() {
        let response: PeopleResponse = serde_json::from_str(
            r#"{"people":[{"id":"person-1","name":"Ada","thumbnailPath":"/person-1.jpg"},{"id":"person-2","name":null}],"total":2}"#,
        )
        .expect("Immich PersonResponseDto should deserialize");

        assert_eq!(response.people[0].id, "person-1");
        assert_eq!(
            response.people[0].thumbnail_path.as_deref(),
            Some("/person-1.jpg")
        );
        assert_eq!(response.people[1].thumbnail_path, None);
    }
}
