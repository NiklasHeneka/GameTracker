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

/// How long a game takes, in seconds, as IGDB's players reported it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeToBeat {
    pub hastily: Option<i64>,
    pub normally: Option<i64>,
    pub completely: Option<i64>,
    /// How many players submitted a time.
    pub count: i64,
    /// Whether the numbers are worth printing. See [`TimeToBeat::is_trusted`].
    pub trusted: bool,
}

impl TimeToBeat {
    /// A handful of submissions produces nonsense — Monster Hunter: World has
    /// four, and reports the completionist run as *shorter* than the normal
    /// one. So a figure is only shown when enough people agree and the three
    /// times are in the order they must be.
    pub fn is_trusted(
        hastily: Option<i64>,
        normally: Option<i64>,
        completely: Option<i64>,
        count: i64,
    ) -> bool {
        const MIN_SUBMISSIONS: i64 = 3;

        let Some(normally) = normally else {
            return false;
        };
        if count < MIN_SUBMISSIONS {
            return false;
        }
        if hastily.is_some_and(|h| h > normally) {
            return false;
        }
        if completely.is_some_and(|c| c < normally) {
            return false;
        }
        true
    }

    /// `None` when IGDB has no row for the game at all.
    pub fn new(
        hastily: Option<i64>,
        normally: Option<i64>,
        completely: Option<i64>,
        count: Option<i64>,
    ) -> Option<Self> {
        let count = count?;
        if count == 0 {
            return None;
        }
        Some(Self {
            hastily,
            normally,
            completely,
            count,
            trusted: Self::is_trusted(hastily, normally, completely, count),
        })
    }
}

/// Steam's aggregate verdict, as the storefront phrases it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamReview {
    /// 1-9. Rows only ever see a score Steam was willing to summarise.
    pub score: i64,
    /// "Overwhelmingly Positive", "Mixed", …
    pub desc: String,
    pub total: i64,
}

impl SteamReview {
    /// `None` unless Steam gave a phrase rather than a bare count.
    pub fn new(score: Option<i64>, desc: Option<String>, total: Option<i64>) -> Option<Self> {
        let score = score.filter(|s| *s > 0)?;
        let desc = desc.filter(|d| !d.is_empty())?;
        Some(Self {
            score,
            desc,
            total: total.unwrap_or(0),
        })
    }
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
    /// IGDB's aggregate score out of 100.
    pub igdb_rating: Option<f64>,
    /// `None` until IGDB has been asked, or when it has nothing to say.
    pub time_to_beat: Option<TimeToBeat>,
    /// `None` for anything with no Steam listing, or too few reviews to judge.
    pub steam_review: Option<SteamReview>,
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

    /// Seconds, straight from IGDB.
    const HOUR: i64 = 3600;

    #[test]
    fn a_bare_review_count_is_not_a_verdict() {
        // Steam sends score 0 with a raw count like "7 user reviews" when it
        // has too few to summarise. That is not a phrase worth showing.
        assert!(SteamReview::new(Some(0), Some("7 user reviews".into()), Some(7)).is_none());
        assert!(SteamReview::new(None, None, None).is_none());
        assert!(SteamReview::new(Some(8), Some(String::new()), Some(100)).is_none());

        let ok = SteamReview::new(
            Some(9),
            Some("Overwhelmingly Positive".into()),
            Some(561_845),
        )
        .expect("a real verdict");
        assert_eq!(ok.desc, "Overwhelmingly Positive");
        assert_eq!(ok.total, 561_845);
    }

    #[test]
    fn a_well_attested_game_is_trusted() {
        // Hollow Knight: 17 h / 36 h / 73 h from 31 players.
        assert!(TimeToBeat::is_trusted(
            Some(60_102),
            Some(129_814),
            Some(263_537),
            31
        ));
    }

    #[test]
    fn a_completionist_run_shorter_than_the_normal_one_is_not_trusted() {
        // Monster Hunter: World, from four submissions — IGDB reports the
        // completionist time as *shorter* than the normal one, which cannot
        // be true and is why the count alone is not enough.
        assert!(!TimeToBeat::is_trusted(
            Some(126_000),
            Some(711_000),
            Some(432_000),
            4
        ));
    }

    #[test]
    fn a_hasty_run_longer_than_the_normal_one_is_not_trusted() {
        assert!(!TimeToBeat::is_trusted(
            Some(40 * HOUR),
            Some(20 * HOUR),
            Some(60 * HOUR),
            50
        ));
    }

    #[test]
    fn too_few_submissions_are_not_trusted() {
        assert!(!TimeToBeat::is_trusted(
            Some(5 * HOUR),
            Some(10 * HOUR),
            Some(20 * HOUR),
            2
        ));
    }

    #[test]
    fn missing_fields_are_fine_as_long_as_the_normal_time_is_there() {
        // Plenty of games have no hastily figure at all.
        assert!(TimeToBeat::is_trusted(None, Some(9 * HOUR), None, 8));
        // But the normal time is the one shown, so without it there is
        // nothing to print.
        assert!(!TimeToBeat::is_trusted(Some(HOUR), None, Some(HOUR), 99));
    }

    #[test]
    fn no_submissions_means_no_figure_at_all() {
        // Stamped as "asked, nothing there" so it is not re-requested.
        assert!(TimeToBeat::new(None, None, None, Some(0)).is_none());
        // Never asked.
        assert!(TimeToBeat::new(None, Some(HOUR), None, None).is_none());
    }

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
