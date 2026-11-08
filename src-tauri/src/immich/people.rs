use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecognizedPerson {
    pub id: String,
    pub name: Option<String>,
    pub asset_count: u64,
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
            return Err(format!(
                "Unable to load recognized people: {}",
                response_status_error(response).await?
            ));
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
