use serde::Serialize;

/// One shop's current offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    pub shop: String,
    pub platform_family: String,
    pub currency: String,
    pub price: f64,
    pub regular: Option<f64>,
    pub cut: i64,
    pub url: Option<String>,
    /// Unix seconds when the sale ends, when the source knows it.
    pub sale_expiry: Option<i64>,
    /// itad | cheapshark | manual
    pub source: String,
    /// True when this offer matches the all-time low.
    pub is_all_time_low: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceGroup {
    /// pc | playstation | nintendo | xbox
    pub family: String,
    pub rows: Vec<PriceRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceLow {
    /// all | y1 | m3
    pub scope: String,
    pub shop: Option<String>,
    pub currency: String,
    pub price: f64,
    pub cut: i64,
    pub occurred_at: Option<i64>,
}

/// The most recent discount that has already ended.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSale {
    pub price: f64,
    pub currency: String,
    pub cut: i64,
    pub occurred_at: i64,
    pub shop: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub ts: i64,
    pub price: f64,
    pub cut: i64,
}

/// How often the game has been available at a discount.
///
/// Deliberately *not* a prediction of the next sale. With twenty shops running
/// staggered promotions, a popular game is discounted somewhere on most days,
/// so "how often does it go on sale" has no meaningful answer. What is both
/// well-defined and useful is how much of the observed window had *some*
/// discount, and how deep the best one went.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountPattern {
    pub days_observed: i64,
    pub days_discounted: i64,
    /// Share of observed days with any discount, 0-100.
    pub share_percent: i64,
    pub deepest_cut: i64,
}

/// What a game cost during a previous run of a storewide sale.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastEventPrice {
    pub event: String,
    pub start: i64,
    pub end: i64,
    pub best_price: Option<f64>,
    pub best_cut: Option<i64>,
    pub currency: String,
    /// False when no price was recorded in that window at all — different from
    /// "it was not discounted".
    pub had_data: bool,
}

/// The next known storewide sale at one shop, and how this game fared in the
/// previous one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSaleOutlook {
    pub shop: String,
    pub event: String,
    pub next_start: i64,
    pub next_end: i64,
    /// True when the store published the dates; otherwise they are estimated
    /// from previous years.
    pub confirmed: bool,
    pub live_now: bool,
    pub days_away: i64,
    pub last_event: Option<PastEventPrice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceOverview {
    pub game_id: i64,
    pub country: String,
    pub groups: Vec<PriceGroup>,
    pub lows: Vec<PriceLow>,
    pub last_sale: Option<LastSale>,
    pub history: Vec<HistoryPoint>,
    pub pattern: Option<DiscountPattern>,
    pub sale_outlook: Vec<StoreSaleOutlook>,
    pub fetched_at: Option<i64>,
    /// Set when the data is older than the refresh interval.
    pub stale: bool,
    /// User-facing caveat, e.g. CheapShark quoting USD.
    pub note: Option<String>,
}

/// A discounted wishlist game, for the Deals view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DealRow {
    pub entry_id: i64,
    pub game_id: i64,
    pub name: String,
    pub cover_image_id: Option<String>,
    pub shop: String,
    pub currency: String,
    pub price: f64,
    pub regular: Option<f64>,
    pub cut: i64,
    pub url: Option<String>,
    pub sale_expiry: Option<i64>,
    pub all_time_low: Option<f64>,
    /// price ÷ all-time low. 1.0 means it is at its best price ever.
    pub vs_all_time_low: Option<f64>,
    pub target_price: Option<f64>,
    pub meets_target: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub checked: usize,
    pub updated: usize,
    pub failed: usize,
    pub alerts: usize,
    pub message: Option<String>,
}
