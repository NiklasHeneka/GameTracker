use serde::{Deserialize, Deserializer, Serialize};

/// Distinguish "field absent" from "field explicitly null".
///
/// Plain `Option<Option<T>>` cannot: serde maps a JSON `null` onto the *outer*
/// `Option`, yielding `None` — indistinguishable from the key being missing.
/// Clearing a rating or a note would then silently do nothing. Wrapping the
/// inner parse in `Some` gives absent → `None`, null → `Some(None)`,
/// value → `Some(Some(v))`.
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::deserialize(de).map(Some)
}

/// A game as the library grid needs it: enough to draw a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub igdb_id: i64,
    pub name: String,
    pub cover_image_id: Option<String>,
    pub first_release: Option<i64>,
    pub steam_appid: Option<i64>,
    pub genres: Vec<String>,
    pub platforms: Vec<PlatformRef>,
}

/// Everything the detail drawer shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    #[serde(flatten)]
    pub summary: GameSummary,
    pub summary_text: Option<String>,
    pub artwork_image_id: Option<String>,
    pub igdb_rating: Option<f64>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRef {
    pub id: i64,
    pub name: String,
    pub abbreviation: Option<String>,
    /// pc | playstation | nintendo | xbox | other — routes price lookups.
    pub family: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub igdb_id: i64,
    pub name: String,
    pub cover_image_id: Option<String>,
    pub release_year: Option<i32>,
    pub platforms: Vec<String>,
    pub rating_count: i64,
    /// Already tracked — the UI shows "In library" instead of an add button.
    pub in_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub id: i64,
    pub game: GameSummary,
    pub owned: bool,
    pub status: String,
    pub priority: i64,
    pub own_platform: Option<String>,
    pub target_price: Option<f64>,
    pub price_at_add: Option<f64>,
    pub purchase_price: Option<f64>,
    pub purchase_date: Option<i64>,
    pub purchase_store: Option<String>,
    pub hours_played: Option<f64>,
    pub my_rating: Option<i64>,
    pub notes: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

/// Partial update of a library entry. Every field is optional; `null` for a
/// nullable column clears it, which is why these are doubly wrapped.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryPatch {
    pub owned: Option<bool>,
    pub status: Option<String>,
    #[serde(deserialize_with = "double_option")]
    pub own_platform: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    pub target_price: Option<Option<f64>>,
    #[serde(deserialize_with = "double_option")]
    pub purchase_price: Option<Option<f64>>,
    #[serde(deserialize_with = "double_option")]
    pub purchase_date: Option<Option<i64>>,
    #[serde(deserialize_with = "double_option")]
    pub purchase_store: Option<Option<String>>,
    #[serde(deserialize_with = "double_option")]
    pub hours_played: Option<Option<f64>>,
    #[serde(deserialize_with = "double_option")]
    pub my_rating: Option<Option<i64>>,
    #[serde(deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

pub const STATUSES: [&str; 4] = ["want", "playing", "finished", "dropped"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_null_and_value_are_three_distinct_states() {
        let absent: EntryPatch = serde_json::from_str(r#"{"owned":true}"#).unwrap();
        assert_eq!(absent.my_rating, None, "absent must leave the column alone");

        let cleared: EntryPatch = serde_json::from_str(r#"{"myRating":null}"#).unwrap();
        assert_eq!(
            cleared.my_rating,
            Some(None),
            "explicit null must clear the column"
        );

        let set: EntryPatch = serde_json::from_str(r#"{"myRating":9}"#).unwrap();
        assert_eq!(set.my_rating, Some(Some(9)));
    }

    #[test]
    fn every_nullable_field_can_be_cleared() {
        let p: EntryPatch = serde_json::from_str(
            r#"{"ownPlatform":null,"targetPrice":null,"purchasePrice":null,
                "purchaseDate":null,"purchaseStore":null,"hoursPlayed":null,
                "myRating":null,"notes":null}"#,
        )
        .unwrap();
        assert_eq!(p.own_platform, Some(None));
        assert_eq!(p.target_price, Some(None));
        assert_eq!(p.purchase_price, Some(None));
        assert_eq!(p.purchase_date, Some(None));
        assert_eq!(p.purchase_store, Some(None));
        assert_eq!(p.hours_played, Some(None));
        assert_eq!(p.my_rating, Some(None));
        assert_eq!(p.notes, Some(None));
    }
}
