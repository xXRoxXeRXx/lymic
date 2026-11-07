use super::client::ImmichClient;
use super::response::{
    read_response_body_limited, response_status_error, MAX_SUCCESS_RESPONSE_BYTES,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBucket {
    pub time_bucket: String,
    pub count: u64,
}

impl ImmichClient {
    pub async fn get_timeline_buckets(&self) -> Result<Vec<TimelineBucket>, String> {
        let response = self
            .client
            .get(self.endpoint_url("timeline/buckets"))
            .query(&[("size", "MONTH")])
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|error| format!("Failed to fetch timeline buckets: {error}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch timeline buckets: {}",
                response_status_error(response).await?
            ));
        }

        let body = read_response_body_limited(response, MAX_SUCCESS_RESPONSE_BYTES).await?;
        let mut buckets: Vec<TimelineBucket> = serde_json::from_slice(&body)
            .map_err(|error| format!("Invalid timeline buckets response: {error}"))?;
        buckets.sort_by(|a, b| b.time_bucket.cmp(&a.time_bucket));
        Ok(buckets)
    }
}
