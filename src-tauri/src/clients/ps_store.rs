use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::util::rate_limit::RateLimiter;

/// Shipped with the app; a copy next to `.env` wins so a hash rotation can be
/// fixed by editing a file rather than rebuilding.
const BUNDLED: &str = include_str!("../../ps-store-queries.json");

/// Sony's store front-end identifies as a browser and the endpoint is picky;
/// a generic agent invites being blocked.
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Debug, Deserialize)]
struct Queries {
    endpoint: String,
    operations: HashMap<String, Operation>,
}

#[derive(Debug, Clone, Deserialize)]
struct Operation {
    #[serde(rename = "operationName")]
    operation_name: String,
    #[serde(rename = "sha256Hash")]
    sha256_hash: String,
}

// ── Wire types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Envelope {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GqlError>,
}

#[derive(Debug, Deserialize)]
struct GqlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct Data {
    #[serde(rename = "conceptRetrieve")]
    concept: Option<Concept>,
}

#[derive(Debug, Deserialize)]
struct Concept {
    #[serde(rename = "defaultProduct")]
    default_product: Option<Product>,
}

#[derive(Debug, Deserialize)]
struct Product {
    id: Option<String>,
    price: Option<RawPrice>,
}

/// Money arrives as integer minor units (`5999` = €59.99) alongside a
/// preformatted string. The integers are used; the strings are localised and
/// would need parsing per region.
#[derive(Debug, Deserialize)]
struct RawPrice {
    #[serde(rename = "basePriceValue")]
    base_price_value: Option<i64>,
    #[serde(rename = "discountedValue")]
    discounted_value: Option<i64>,
    #[serde(rename = "currencyCode")]
    currency_code: Option<String>,
    /// When the current discount ends; null outside a sale.
    #[serde(rename = "endTime")]
    end_time: Option<String>,
    #[serde(rename = "isFree")]
    #[serde(default)]
    is_free: bool,
}

/// A PlayStation Store offer for one game.
#[derive(Debug, Clone)]
pub struct PsPrice {
    pub price: f64,
    pub regular: Option<f64>,
    pub cut: i64,
    pub currency: String,
    /// Unix seconds when the discount ends, if one is running.
    pub expiry: Option<i64>,
    pub url: String,
}

/// Map an ISO country to the `language-COUNTRY` locale the store expects.
/// Unknown countries fall back to English in that country, which the store
/// accepts for every region it serves.
pub fn locale_for(country: &str) -> String {
    let c = country.to_uppercase();
    let language = match c.as_str() {
        "DE" | "AT" => "de",
        "FR" => "fr",
        "ES" => "es",
        "IT" => "it",
        "NL" => "nl",
        "PL" => "pl",
        "PT" => "pt",
        "CH" => "de",
        _ => "en",
    };
    format!("{language}-{c}")
}

pub struct PsStore {
    http: reqwest::Client,
    queries: Queries,
    limiter: RateLimiter,
}

impl PsStore {
    pub fn new(config_dir: Option<&Path>) -> Result<Self> {
        let queries = load(config_dir);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(BROWSER_UA)
            .gzip(true)
            .build()
            .map_err(|e| AppError::Config(format!("could not build HTTP client: {e}")))?;

        Ok(Self {
            http,
            queries,
            limiter: RateLimiter::per_second(2),
        })
    }

    /// Current price for a concept id, in the region implied by `locale`
    /// (e.g. `de-DE`).
    pub async fn price(&self, concept_id: &str, locale: &str) -> Result<Option<PsPrice>> {
        let op = self
            .queries
            .operations
            .get("pricingByConceptId")
            .ok_or_else(|| {
                AppError::Config(
                    "ps-store-queries.json has no 'pricingByConceptId' operation".into(),
                )
            })?;

        self.limiter.acquire().await;

        let variables = format!(r#"{{"conceptId":"{concept_id}"}}"#);
        let extensions = format!(
            r#"{{"persistedQuery":{{"version":1,"sha256Hash":"{}"}}}}"#,
            op.sha256_hash
        );

        let res = self
            .http
            .get(&self.queries.endpoint)
            .header("x-psn-store-locale-override", locale)
            // Apollo refuses GET operations without one of these, treating them
            // as possible cross-site requests.
            .header("x-apollo-operation-name", &op.operation_name)
            .header("apollo-require-preflight", "true")
            .query(&[
                ("operationName", op.operation_name.as_str()),
                ("variables", &variables),
                ("extensions", &extensions),
            ])
            .send()
            .await?;

        let status = res.status();
        let body = res.text().await?;

        if !status.is_success() {
            // A rotated hash is the most likely failure and has a specific fix.
            if body.contains("not whitelisted") {
                return Err(AppError::Config(
                    "PlayStation price lookups need updated query hashes — Sony has rotated \
                     them. Edit ps-store-queries.json (see the notes in that file)."
                        .into(),
                ));
            }
            return Err(AppError::Api {
                service: "PlayStation Store",
                status: status.as_u16(),
                body: body.chars().take(300).collect(),
            });
        }

        let envelope: Envelope = serde_json::from_str(&body)?;
        if let Some(err) = envelope.errors.first() {
            if err.message.contains("not whitelisted") {
                return Err(AppError::Config(
                    "PlayStation price lookups need updated query hashes — Sony has rotated \
                     them. Edit ps-store-queries.json (see the notes in that file)."
                        .into(),
                ));
            }
            return Err(AppError::Api {
                service: "PlayStation Store",
                status: 200,
                body: err.message.chars().take(300).collect(),
            });
        }

        let Some(product) = envelope
            .data
            .and_then(|d| d.concept)
            .and_then(|c| c.default_product)
        else {
            return Ok(None);
        };

        let Some(raw) = product.price else {
            return Ok(None);
        };

        Ok(to_price(&raw, concept_id, locale, product.id.as_deref()))
    }
}

fn to_price(
    raw: &RawPrice,
    concept_id: &str,
    locale: &str,
    _product: Option<&str>,
) -> Option<PsPrice> {
    // A giveaway can arrive with no base price at all; treat it as zero rather
    // than dropping the offer.
    let base = match raw.base_price_value {
        Some(v) => v,
        None if raw.is_free => 0,
        None => return None,
    };
    // A free game still has a price of zero; only a missing value is unusable.
    let discounted = raw.discounted_value.unwrap_or(base);
    if base < 0 || discounted < 0 {
        return None;
    }

    let cut = if base > 0 && discounted < base {
        (((base - discounted) as f64 / base as f64) * 100.0).round() as i64
    } else {
        0
    };

    // store.playstation.com wants a language-COUNTRY path segment, lowercased.
    let path_locale = locale.to_lowercase();

    Some(PsPrice {
        price: discounted as f64 / 100.0,
        regular: (cut > 0).then(|| base as f64 / 100.0),
        cut,
        currency: raw.currency_code.clone().unwrap_or_else(|| "EUR".into()),
        expiry: raw.end_time.as_deref().and_then(parse_ps_time),
        url: format!("https://store.playstation.com/{path_locale}/concept/{concept_id}"),
    })
}

/// PS Store timestamps look like `2026-10-28T23:59:00Z`, occasionally with
/// milliseconds. Reuse the RFC 3339 parser, trimming any fractional seconds.
fn parse_ps_time(s: &str) -> Option<i64> {
    let trimmed = match s.find('.') {
        Some(dot) => {
            let tail_start = s[dot..].find(['Z', '+', '-'])? + dot;
            format!("{}{}", &s[..dot], &s[tail_start..])
        }
        None => s.to_string(),
    };
    crate::clients::itad::parse_timestamp(&trimmed)
}

fn load(config_dir: Option<&Path>) -> Queries {
    if let Some(dir) = config_dir {
        let path = dir.join("ps-store-queries.json");
        if path.is_file() {
            match std::fs::read_to_string(&path).map(|s| serde_json::from_str::<Queries>(&s)) {
                Ok(Ok(q)) => {
                    log::info!("using PlayStation query hashes from {}", path.display());
                    return q;
                }
                Ok(Err(e)) => log::warn!(
                    "{} is not valid JSON ({e}); using the bundled hashes",
                    path.display()
                ),
                Err(e) => log::warn!(
                    "could not read {} ({e}); using the bundled hashes",
                    path.display()
                ),
            }
        }
    }
    serde_json::from_str(BUNDLED).expect("the bundled PlayStation query file must parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(base: Option<i64>, discounted: Option<i64>, end: Option<&str>, free: bool) -> RawPrice {
        RawPrice {
            base_price_value: base,
            discounted_value: discounted,
            currency_code: Some("EUR".into()),
            end_time: end.map(|s| s.to_string()),
            is_free: free,
        }
    }

    #[test]
    fn the_bundled_query_file_parses_and_has_the_pricing_operation() {
        let q = load(None);
        assert!(q.endpoint.starts_with("https://"));
        let op = q
            .operations
            .get("pricingByConceptId")
            .expect("pricing operation");
        assert_eq!(op.operation_name, "metGetPricingDataByConceptId");
        assert_eq!(
            op.sha256_hash.len(),
            64,
            "a sha256 hash is 64 hex characters"
        );
        assert!(op.sha256_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn full_price_reports_no_discount() {
        // Exactly what Elden Ring returned live: base == discounted.
        let p = to_price(
            &raw(Some(5999), Some(5999), None, false),
            "10000333",
            "de-DE",
            None,
        )
        .unwrap();
        assert_eq!(p.cut, 0);
        assert!((p.price - 59.99).abs() < 1e-9);
        assert_eq!(p.regular, None, "no strikethrough when nothing is off");
        assert_eq!(p.expiry, None);
        assert!(p.url.contains("/de-de/concept/10000333"));
    }

    #[test]
    fn a_discount_yields_a_cut_regular_price_and_expiry() {
        let p = to_price(
            &raw(Some(5999), Some(2999), Some("2026-10-28T23:59:00Z"), false),
            "10000333",
            "de-DE",
            None,
        )
        .unwrap();
        assert_eq!(p.cut, 50);
        assert!((p.price - 29.99).abs() < 1e-9);
        assert_eq!(p.regular, Some(59.99));
        assert_eq!(
            p.expiry,
            crate::clients::itad::parse_timestamp("2026-10-28T23:59:00Z")
        );
    }

    #[test]
    fn fractional_seconds_in_the_end_time_are_tolerated() {
        let with = parse_ps_time("2026-10-28T23:59:00.000Z");
        let without = parse_ps_time("2026-10-28T23:59:00Z");
        assert_eq!(with, without);
        assert!(with.is_some());
    }

    #[test]
    fn a_free_game_is_priced_at_zero_rather_than_dropped() {
        let p = to_price(&raw(None, Some(0), None, true), "1", "de-DE", None).unwrap();
        assert_eq!(p.price, 0.0);
        assert_eq!(p.cut, 0);
    }

    #[test]
    fn a_missing_price_is_not_invented() {
        assert!(to_price(&raw(None, None, None, false), "1", "de-DE", None).is_none());
    }

    #[test]
    fn locales_map_to_the_regions_the_store_serves() {
        assert_eq!(locale_for("DE"), "de-DE");
        assert_eq!(locale_for("AT"), "de-AT");
        assert_eq!(locale_for("GB"), "en-GB");
        assert_eq!(locale_for("US"), "en-US");
        // Unknown country still produces a well-formed locale.
        assert_eq!(locale_for("se"), "en-SE");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the real PlayStation Store:
    ///   cargo test --lib live_ps -- --ignored --nocapture
    ///
    /// Sony rotates the persisted-query hashes whenever the storefront is
    /// redeployed; this test is how that gets noticed.
    #[tokio::test]
    #[ignore]
    async fn live_ps_price_for_elden_ring() {
        let store = PsStore::new(None).unwrap();
        let price = store
            .price("10000333", "de-DE")
            .await
            .expect("the request itself must succeed")
            .expect("Elden Ring has a PlayStation listing");

        assert_eq!(price.currency, "EUR", "locale override did not apply");
        assert!(
            price.price > 0.0 && price.price < 200.0,
            "implausible: {}",
            price.price
        );
        assert!(price.cut >= 0 && price.cut <= 100);
        println!(
            "Elden Ring PS: {:.2} {} cut={}% expiry={:?}",
            price.price, price.currency, price.cut, price.expiry
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_ps_rotated_hash_produces_an_actionable_message() {
        // A deliberately wrong hash must surface as "edit the file", not a
        // generic HTTP error.
        let mut store = PsStore::new(None).unwrap();
        store.queries.operations.insert(
            "pricingByConceptId".into(),
            Operation {
                operation_name: "metGetPricingDataByConceptId".into(),
                sha256_hash: "0".repeat(64),
            },
        );
        let err = store.price("10000333", "de-DE").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ps-store-queries.json"), "unhelpful: {msg}");
        println!("rotated hash -> {msg}");
    }
}
