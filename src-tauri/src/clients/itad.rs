use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::util::rate_limit::RateLimiter;

const API: &str = "https://api.isthereanydeal.com";

// ── Wire types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

#[derive(Debug, Deserialize)]
pub struct Shop {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct LookupGame {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct LookupResponse {
    pub found: bool,
    pub game: Option<LookupGame>,
}

#[derive(Debug, Deserialize)]
pub struct Deal {
    pub shop: Shop,
    pub price: Money,
    pub regular: Option<Money>,
    #[serde(default)]
    pub cut: i64,
    pub url: Option<String>,
    /// When the sale ends. ITAD knows this for roughly half of live deals.
    pub expiry: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GamePrices {
    /// Needed to match responses to requests once several ids are batched.
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    pub deals: Vec<Deal>,
}

#[derive(Debug, Deserialize)]
pub struct Low {
    pub shop: Option<Shop>,
    pub price: Money,
    #[allow(dead_code)] // the regular price at the time of the low
    pub regular: Option<Money>,
    #[serde(default)]
    pub cut: i64,
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryLow {
    #[allow(dead_code)]
    pub id: String,
    pub low: Option<Low>,
}

/// One price change. `shop` sits beside `deal`, not inside it.
#[derive(Debug, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub shop: Option<Shop>,
    pub deal: Option<HistoryDeal>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryDeal {
    pub price: Money,
    pub regular: Option<Money>,
    #[serde(default)]
    pub cut: i64,
}

/// Region-aware PC prices, sale expiry and price history.
pub struct Itad {
    http: reqwest::Client,
    key: String,
    limiter: RateLimiter,
}

impl Itad {
    pub fn new(key: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .user_agent(crate::clients::user_agent())
            .gzip(true)
            .build()
            .map_err(|e| AppError::Config(format!("could not build HTTP client: {e}")))?;

        Ok(Self {
            http,
            key,
            // 1000 requests / 5 minutes; stay well inside it.
            limiter: RateLimiter::per_second(3),
        })
    }

    async fn send<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        self.limiter.acquire().await;
        let res = req.send().await?;
        let status = res.status();

        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            let hint = if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                " — check ITAD_API_KEY in your .env"
            } else {
                ""
            };
            return Err(AppError::Api {
                service: "IsThereAnyDeal",
                status: status.as_u16(),
                body: format!("{}{hint}", body.chars().take(300).collect::<String>()),
            });
        }

        Ok(res.json().await?)
    }

    /// Steam appid → ITAD's own game id. The universal join key again.
    pub async fn lookup_by_appid(&self, appid: i64) -> Result<Option<String>> {
        let res: LookupResponse = self
            .send(self.http.get(format!("{API}/games/lookup/v1")).query(&[
                ("key", self.key.as_str()),
                ("appid", &appid.to_string()),
            ]))
            .await?;
        Ok(if res.found { res.game.map(|g| g.id) } else { None })
    }

    pub async fn lookup_by_title(&self, title: &str) -> Result<Option<String>> {
        let res: LookupResponse = self
            .send(
                self.http
                    .get(format!("{API}/games/lookup/v1"))
                    .query(&[("key", self.key.as_str()), ("title", title)]),
            )
            .await?;
        Ok(if res.found { res.game.map(|g| g.id) } else { None })
    }

    /// Current prices across shops. Accepts up to 200 ids per call.
    pub async fn prices(&self, ids: &[String], country: &str) -> Result<Vec<GamePrices>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.send(
            self.http
                .post(format!("{API}/games/prices/v3"))
                .query(&[
                    ("key", self.key.as_str()),
                    ("country", country),
                    ("deals", "false"),
                ])
                .json(&ids),
        )
        .await
    }

    pub async fn history_lows(&self, ids: &[String], country: &str) -> Result<Vec<HistoryLow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.send(
            self.http
                .post(format!("{API}/games/historylow/v1"))
                .query(&[("key", self.key.as_str()), ("country", country)])
                .json(&ids),
        )
        .await
    }

    /// Price change log. Powers the sparkline, "last on sale" and the cadence
    /// estimate.
    ///
    /// **`since` matters enormously**: without it ITAD returns only about three
    /// months (12 entries for Hollow Knight), which is far too short to say
    /// anything about how often a game goes on sale. Asking for several years
    /// returns hundreds of entries instead.
    pub async fn history(&self, id: &str, country: &str, since_years: i64) -> Result<Vec<HistoryEntry>> {
        let since = format_date(crate::db::now() - since_years * 365 * 86_400);
        self.send(self.http.get(format!("{API}/games/history/v2")).query(&[
            ("key", self.key.as_str()),
            ("country", country),
            ("id", id),
            ("since", &since),
        ]))
        .await
    }

}

/// Render unix seconds as an RFC 3339 UTC date, which is what `since` expects.
pub fn format_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T00:00:00Z")
}

/// ITAD timestamps are RFC 3339 (`2026-08-13T17:00:00+02:00`). Parsed by hand
/// to avoid pulling in chrono for one format.
pub fn parse_timestamp(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);

    // Days from civil date (Howard Hinnant's algorithm).
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let mut ts = days * 86_400 + h * 3600 + mi * 60 + sec;

    // Offset suffix: 'Z', or ±HH:MM.
    if let Some(rest) = s.get(19..) {
        let rest = rest.trim();
        if let Some(sign) = rest.chars().next() {
            if sign == '+' || sign == '-' {
                let oh: i64 = rest.get(1..3).and_then(|v| v.parse().ok()).unwrap_or(0);
                let om: i64 = rest.get(4..6).and_then(|v| v.parse().ok()).unwrap_or(0);
                let offset = oh * 3600 + om * 60;
                ts += if sign == '+' { -offset } else { offset };
            }
        }
    }
    Some(ts)
}

#[cfg(test)]
mod tests {
    use super::parse_timestamp;

    #[test]
    fn parses_real_itad_timestamps() {
        // Values taken from live responses, cross-checked against `date -u`.
        assert_eq!(parse_timestamp("2020-11-27T15:24:08+01:00"), Some(1606487048));
        assert_eq!(parse_timestamp("2026-08-13T17:00:00+02:00"), Some(1786633200));
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2000-03-01T00:00:00Z"), Some(951868800));
    }

    #[test]
    fn offset_sign_is_applied_in_the_right_direction() {
        // Same wall clock, opposite offsets: the negative offset is later in UTC.
        let plus = parse_timestamp("2026-01-01T12:00:00+02:00").unwrap();
        let minus = parse_timestamp("2026-01-01T12:00:00-02:00").unwrap();
        assert_eq!(minus - plus, 4 * 3600);
    }

    #[test]
    fn leap_years_and_month_boundaries_hold() {
        // 29 Feb exists in 2024; the day after is 1 March.
        let feb29 = parse_timestamp("2024-02-29T00:00:00Z").unwrap();
        let mar01 = parse_timestamp("2024-03-01T00:00:00Z").unwrap();
        assert_eq!(mar01 - feb29, 86_400);
    }

    #[test]
    fn rejects_input_that_is_too_short() {
        assert_eq!(parse_timestamp("2026-08-13"), None);
        assert_eq!(parse_timestamp(""), None);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the real IsThereAnyDeal API:
    ///   cargo test --lib live_itad -- --ignored --nocapture
    fn client() -> Itad {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".env");
        dotenvy::from_path(&root).ok();
        Itad::new(std::env::var("ITAD_API_KEY").expect("ITAD_API_KEY in .env")).unwrap()
    }

    const HOLLOW_KNIGHT_APPID: i64 = 367520;

    #[tokio::test]
    #[ignore]
    async fn live_itad_full_pipeline() {
        let itad = client();

        let uuid = itad
            .lookup_by_appid(HOLLOW_KNIGHT_APPID)
            .await
            .unwrap()
            .expect("Steam appid resolves to an ITAD id");
        println!("uuid: {uuid}");

        let ids = vec![uuid.clone()];

        let prices = itad.prices(&ids, "DE").await.unwrap();
        let deals = &prices.first().expect("a price entry").deals;
        assert!(!deals.is_empty(), "no shops returned");
        // Region actually applied — DE must not come back in USD.
        assert_eq!(deals[0].price.currency, "EUR", "country parameter ignored?");
        println!("{} shops, cheapest {:?}", deals.len(), deals[0].price.amount);

        let low = itad
            .history_lows(&ids, "DE")
            .await
            .unwrap()
            .into_iter()
            .next()
            .and_then(|l| l.low)
            .expect("an all-time low");
        assert!(low.price.amount > 0.0);
        assert!(low.timestamp.as_deref().and_then(parse_timestamp).is_some());
        println!("all-time low {:.2} {} at {:?}", low.price.amount, low.price.currency,
                 low.shop.map(|s| s.name));

        let history = itad.history(&uuid, "DE", 5).await.unwrap();
        assert!(!history.is_empty(), "no price history");
        // Every entry must carry a parseable timestamp, or the sparkline and
        // "last on sale" silently lose data.
        for h in &history {
            assert!(parse_timestamp(&h.timestamp).is_some(), "bad ts {}", h.timestamp);
        }
        let discounts = history.iter().filter(|h| h.deal.as_ref().map(|d| d.cut).unwrap_or(0) > 0).count();
        println!("{} history points, {} of them discounts", history.len(), discounts);

        // Without `since` ITAD returns roughly three months, far too short to
        // describe how often a game is discounted.
        let default_window = itad.history(&uuid, "DE", 0).await.unwrap();
        assert!(
            history.len() > default_window.len() * 3,
            "`since` should unlock far more history: {} vs {}",
            history.len(),
            default_window.len()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_itad_rejects_a_bad_key_clearly() {
        let bad = Itad::new("definitely-not-a-real-key".into()).unwrap();
        let err = bad.lookup_by_appid(HOLLOW_KNIGHT_APPID).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ITAD_API_KEY"), "unhelpful message: {msg}");
        println!("bad key -> {msg}");
    }
}

#[cfg(test)]
mod date_tests {
    use super::{format_date, parse_timestamp};

    #[test]
    fn format_date_round_trips_through_the_parser() {
        for ts in [0_i64, 951_868_800, 1_606_487_048, 1_786_633_200] {
            let rendered = format_date(ts);
            let midnight = ts - ts.rem_euclid(86_400);
            assert_eq!(
                parse_timestamp(&rendered),
                Some(midnight),
                "{ts} rendered as {rendered}"
            );
        }
    }

    #[test]
    fn format_date_matches_known_dates() {
        assert_eq!(format_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_date(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(format_date(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
