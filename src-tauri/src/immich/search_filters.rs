//! The Search Filters of a Search and the structured `filter` condition each one adds
//! (ADR-0008). A Search Filter is one type here and one line in `SearchFilters::apply`
//! (in `search.rs`); every set filter combines with AND because they are separate keys.

use serde::Deserialize;
use serde_json::{json, Map, Value};

/// A date range as instants. The frontend turns the chosen local days into the start of the
/// first day and the end of the last day (inclusive), so no time zone is needed here.
/// Either bound is optional.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DateRange {
    /// ISO 8601 instant, inclusive.
    pub from: Option<String>,
    /// ISO 8601 instant, inclusive.
    pub to: Option<String>,
}

impl DateRange {
    pub(super) fn apply(&self, filter: &mut Map<String, Value>) {
        let mut bounds = Map::new();
        if let Some(from) = &self.from {
            bounds.insert("gte".to_string(), json!(from));
        }
        if let Some(to) = &self.to {
            bounds.insert("lte".to_string(), json!(to));
        }
        if !bounds.is_empty() {
            filter.insert("localDateTime".to_string(), Value::Object(bounds));
        }
    }
}

/// Restricts a Search to images or to videos; no restriction means all.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MediaType {
    Image,
    Video,
}

impl MediaType {
    pub(super) fn apply(self, filter: &mut Map<String, Value>) {
        let immich_type = match self {
            MediaType::Image => "IMAGE",
            MediaType::Video => "VIDEO",
        };
        filter.insert("type".to_string(), json!({ "eq": immich_type }));
    }
}
