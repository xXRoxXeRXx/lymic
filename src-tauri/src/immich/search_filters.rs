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

/// One exact star level, one to five. A value outside that range is rejected when the
/// request is deserialized, so it never reaches Immich.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u8")]
pub(crate) struct Rating(u8);

impl TryFrom<u8> for Rating {
    type Error = String;

    fn try_from(stars: u8) -> Result<Self, Self::Error> {
        if (1..=5).contains(&stars) {
            Ok(Rating(stars))
        } else {
            Err(format!("a Rating is one to five stars, not {stars}"))
        }
    }
}

impl Rating {
    pub(super) fn apply(self, filter: &mut Map<String, Value>) {
        filter.insert("rating".to_string(), json!({ "eq": self.0 }));
    }
}

/// Favorite: only Favorites.
pub(super) fn apply_favorite(filter: &mut Map<String, Value>) {
    filter.insert("isFavorite".to_string(), json!({ "eq": true }));
}

/// Unfiled Asset: only Remote Assets that are in no Album.
pub(super) fn apply_unfiled(filter: &mut Map<String, Value>) {
    filter.insert("hasAlbums".to_string(), json!({ "eq": false }));
}

/// Visibility is always sent, so the result never depends on Immich's own default: Archived
/// Assets are left out, unless the Archived option is on, which shows only Archived Assets.
pub(super) fn apply_visibility(archived: bool, filter: &mut Map<String, Value>) {
    let visibility = if archived { "archive" } else { "timeline" };
    filter.insert("visibility".to_string(), json!({ "eq": visibility }));
}
