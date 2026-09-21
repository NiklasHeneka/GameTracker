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
    /// When the current discount ends; null outside a sale. The GraphQL query
    /// has sent ISO dates; the store page sends epoch *milliseconds* as a
    /// string. Kept loose and normalised by [`end_time_secs`].
    #[serde(rename = "endTime")]
    #[serde(default)]
    end_time: Option<serde_json::Value>,
    #[serde(rename = "isFree")]
    #[serde(default)]
    is_free: bool,
    /// A price only a PlayStation Plus member gets — often 0, because the game
    /// is in the Plus catalogue. Not something anyone can simply buy.
    #[serde(rename = "isTiedToSubscription")]
    #[serde(default)]
    is_tied_to_subscription: bool,
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

/// A price lookup, and how it was answered.
#[derive(Debug, Clone)]
pub struct PsQuote {
    pub price: Option<PsPrice>,
    /// Sony refused the persisted query's hash, so this came from the store
    /// page instead. Prices keep flowing, but each lookup now downloads a whole
    /// page rather than a few hundred bytes — the hash wants updating.
    pub via_page: bool,
}

/// What the persisted query came back with.
enum Queried {
    Price(Option<PsPrice>),
    /// The hash is no longer whitelisted — Sony has changed the query.
    HashRejected,
}

fn hash_rejected_error() -> AppError {
    AppError::Config(
        "PlayStation prices need an updated query hash — Sony has changed it, and reading \
         the store page instead did not work either. See ps-store-queries.json."
            .into(),
    )
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
    ///
    /// Asks the GraphQL endpoint first — a few hundred bytes. If Sony has
    /// rotated the persisted query's hash, reads the same price out of the
    /// store page, which needs no hash at all, and says so in the result so
    /// the caller can tell the user. Only when both fail is it an error.
    pub async fn price(&self, concept_id: &str, locale: &str) -> Result<PsQuote> {
        match self.query(concept_id, locale).await? {
            Queried::Price(price) => Ok(PsQuote {
                price,
                via_page: false,
            }),
            Queried::HashRejected => {
                log::warn!(
                    "PlayStation query hash rejected; reading concept {concept_id} from the store page"
                );
                match self.price_from_page(concept_id, locale).await {
                    Ok(price) => Ok(PsQuote {
                        price,
                        via_page: true,
                    }),
                    Err(e) => {
                        log::warn!("the store page fallback failed too: {e}");
                        Err(hash_rejected_error())
                    }
                }
            }
        }
    }

    /// The same price, read from `store.playstation.com/{locale}/concept/{id}`.
    ///
    /// The page is server-rendered: its embedded data carries each edition's
    /// price under exactly the field names the GraphQL query uses, and no
    /// persisted-query hash is involved. The cost is size — the page is roughly
    /// 700 KB against a few hundred bytes — which is why it is the fallback and
    /// not the first choice.
    pub async fn price_from_page(&self, concept_id: &str, locale: &str) -> Result<Option<PsPrice>> {
        self.limiter.acquire().await;

        let url = format!(
            "https://store.playstation.com/{}/concept/{concept_id}",
            locale.to_lowercase()
        );
        let res = self.http.get(&url).send().await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            return Err(AppError::Api {
                service: "PlayStation Store page",
                status: status.as_u16(),
                body: body.chars().take(200).collect(),
            });
        }
        parse_page(&body, concept_id, locale)
    }

    /// The persisted GraphQL query.
    async fn query(&self, concept_id: &str, locale: &str) -> Result<Queried> {
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
            // A rotated hash is the most likely failure, and has a fallback.
            if body.contains("not whitelisted") {
                return Ok(Queried::HashRejected);
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
                return Ok(Queried::HashRejected);
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
            return Ok(Queried::Price(None));
        };

        let Some(raw) = product.price else {
            return Ok(Queried::Price(None));
        };

        Ok(Queried::Price(to_price(
            &raw,
            concept_id,
            locale,
            product.id.as_deref(),
        )))
    }
}

// ── The store page ───────────────────────────────────────────────────────

/// Every `<script id="…" type="application/json">` body on a page, with its id.
///
/// Plain string search rather than an HTML parser or a regex dependency: the
/// page is machine-generated, and these tags are the only structure read.
fn json_scripts(html: &str) -> Vec<(&str, &str)> {
    const OPEN: &str = "<script id=\"";
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        let Some(id_end) = after.find('"') else { break };
        let id = &after[..id_end];
        let tail = &after[id_end..];
        let Some(tag_end) = tail.find('>') else { break };
        let body_and_rest = &tail[tag_end + 1..];
        let Some(body_end) = body_and_rest.find("</script>") else {
            break;
        };
        if tail[..tag_end].contains("application/json") {
            out.push((id, &body_and_rest[..body_end]));
        }
        rest = &body_and_rest[body_end..];
    }
    out
}

fn page_changed(what: &str) -> AppError {
    AppError::Api {
        service: "PlayStation Store page",
        status: 200,
        body: format!("the page no longer looks as expected: {what}"),
    }
}

/// The price of a concept's **default edition**, read from its store page.
///
/// A concept page lists every edition and add-on — Elden Ring's shows four
/// prices — so the default product has to be identified first, exactly as the
/// GraphQL query does. Its purchase buttons (`GameCTA`) carry the prices, keyed
/// by SKU, which is the product id plus a suffix.
///
/// PlayStation Plus prices are skipped: a game in the Plus catalogue has a
/// button priced at 0 that only members can use. A regular purchase button is
/// preferred; with none left, there is no price, which is the honest answer.
fn parse_page(html: &str, concept_id: &str, locale: &str) -> Result<Option<PsPrice>> {
    let scripts = json_scripts(html);

    let (_, next_data) = scripts
        .iter()
        .find(|(id, _)| *id == "__NEXT_DATA__")
        .ok_or_else(|| page_changed("no __NEXT_DATA__ block"))?;
    let data: serde_json::Value = serde_json::from_str(next_data)?;
    let apollo = data["props"]["apolloState"]
        .as_object()
        .ok_or_else(|| page_changed("no apolloState"))?;

    let concept_prefix = format!("Concept:{concept_id}:");
    let Some(default_ref) = apollo
        .iter()
        .find(|(key, _)| key.starts_with(&concept_prefix))
        .and_then(|(_, concept)| concept["defaultProduct"]["__ref"].as_str())
    else {
        // No default product — an unreleased or delisted game. Not an error.
        return Ok(None);
    };
    // `Product:EP0700-PPSA04609_00-ELDENRING0000000:de-de`
    let product_id = default_ref
        .split(':')
        .nth(1)
        .ok_or_else(|| page_changed("unexpected product reference"))?;

    // The trailing dash matters: without it, "…ELDENRING0000000" would also
    // match "…ELDENRING0000000X", a different product.
    let needle = format!(":{product_id}-");
    let mut candidates: Vec<(bool, RawPrice)> = Vec::new();

    for (_, body) in &scripts {
        if !body.contains("GameCTA") {
            continue;
        }
        let Ok(block) = serde_json::from_str::<serde_json::Value>(body) else {
            continue;
        };
        let Some(cache) = block["cache"].as_object() else {
            continue;
        };
        for (key, entry) in cache {
            if !key.starts_with("GameCTA:") || !key.contains(&needle) {
                continue;
            }
            let Ok(raw) = serde_json::from_value::<RawPrice>(entry["price"].clone()) else {
                continue;
            };
            if raw.is_tied_to_subscription {
                continue;
            }
            if raw.base_price_value.is_none() && !raw.is_free {
                continue;
            }
            candidates.push((key.ends_with(":OUTRIGHT"), raw));
        }
    }

    // A plain purchase first; any other non-subscription button after it.
    candidates.sort_by_key(|(outright, _)| !outright);
    Ok(candidates
        .first()
        .and_then(|(_, raw)| to_price(raw, concept_id, locale, Some(product_id))))
}

fn to_price(
    raw: &RawPrice,
    concept_id: &str,
    locale: &str,
    _product: Option<&str>,
) -> Option<PsPrice> {
    // A Plus catalogue price is not a price anyone can simply pay. Reporting
    // it made Cities: Skylines look "100% off, €0.00" to everyone.
    if raw.is_tied_to_subscription {
        return None;
    }

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
        expiry: raw.end_time.as_ref().and_then(end_time_secs),
        url: format!("https://store.playstation.com/{path_locale}/concept/{concept_id}"),
    })
}

/// A sale's end time as unix seconds, from whichever shape it arrived in.
///
/// The store page sends epoch milliseconds as a string (`"1790204340000"`);
/// ISO dates are handled too. Anything past 10^10 is taken as milliseconds —
/// in seconds that would be the year 2286.
fn end_time_secs(value: &serde_json::Value) -> Option<i64> {
    let number = match value {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().ok()
        }
        serde_json::Value::String(s) => return parse_ps_time(s),
        _ => None,
    }?;
    Some(if number > 10_000_000_000 {
        number / 1000
    } else {
        number
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
            end_time: end.map(|s| serde_json::Value::String(s.to_string())),
            is_free: free,
            is_tied_to_subscription: false,
        }
    }

    const ELDEN_RING: &str = include_str!("fixtures/ps_concept_elden_ring.html");
    const CYBERPUNK: &str = include_str!("fixtures/ps_concept_cyberpunk.html");
    const CITIES: &str = include_str!("fixtures/ps_concept_cities.html");

    #[test]
    fn the_page_yields_the_default_edition_not_the_deluxe_one() {
        // Four prices on the page: €59.99 standard, €79.99 with the
        // expansion, €99.99 deluxe, €49.99 for the expansion alone. The
        // GraphQL query returns the standard one, so the fallback must too.
        let p = parse_page(ELDEN_RING, "10000333", "de-DE")
            .unwrap()
            .expect("Elden Ring has a price");
        assert!((p.price - 59.99).abs() < 1e-9, "picked {}", p.price);
        assert_eq!(p.cut, 0);
        assert_eq!(p.currency, "EUR");
        assert!(p.url.ends_with("/de-de/concept/10000333"));
    }

    #[test]
    fn the_page_yields_a_discount_and_its_end_date() {
        // Cyberpunk at 60% off, captured live. The page's `endTime` is epoch
        // milliseconds in a string — and it is the one source that has the
        // date at all: the GraphQL query returned null for this same sale.
        let p = parse_page(CYBERPUNK, "234567", "de-DE")
            .unwrap()
            .expect("Cyberpunk has a price");
        assert!((p.price - 19.99).abs() < 1e-9, "picked {}", p.price);
        assert_eq!(p.regular, Some(49.99));
        assert_eq!(p.cut, 60);
        // 1790204340000 ms = 2026-09-23T22:59:00Z.
        assert_eq!(
            p.expiry,
            crate::clients::itad::parse_timestamp("2026-09-23T22:59:00Z")
        );
    }

    #[test]
    fn the_page_skips_ps_plus_prices_and_an_add_on_that_shares_the_page() {
        // Cyberpunk's page also carries a PS Plus button priced at 0 for the
        // same edition, and Phantom Liberty at €34.99. Neither is the answer.
        let p = parse_page(CYBERPUNK, "234567", "de-DE").unwrap().unwrap();
        assert_ne!(
            p.price, 0.0,
            "a PS Plus catalogue price is not a purchase price"
        );
        assert!((p.price - 34.99).abs() > 1e-9, "picked the add-on");
    }

    #[test]
    fn a_game_only_in_ps_plus_has_no_price_rather_than_a_free_one() {
        // Cities: Skylines' default edition is in the Plus catalogue and has
        // no ordinary purchase button. The honest answer is "no price" — not
        // the "€0.00, 100% off" the app used to show everyone.
        assert!(parse_page(CITIES, "224821", "de-DE").unwrap().is_none());
    }

    #[test]
    fn a_ps_plus_price_from_the_query_is_not_a_purchase_price() {
        // Exactly what GraphQL returned for Cities: Skylines.
        let mut plus = raw(Some(3999), Some(0), None, true);
        plus.is_tied_to_subscription = true;
        assert!(to_price(&plus, "224821", "de-DE", None).is_none());
    }

    #[test]
    fn a_page_without_its_data_is_a_clear_error_not_a_missing_price() {
        let err = parse_page("<html><body>maintenance</body></html>", "1", "de-DE").unwrap_err();
        assert!(
            err.to_string().contains("__NEXT_DATA__"),
            "unhelpful: {err}"
        );
    }

    #[test]
    fn end_times_arrive_in_more_than_one_shape() {
        use serde_json::json;
        let expected = crate::clients::itad::parse_timestamp("2026-09-23T22:59:00Z");
        assert_eq!(
            end_time_secs(&json!("1790204340000")),
            expected,
            "ms in a string"
        );
        assert_eq!(
            end_time_secs(&json!(1790204340000_i64)),
            expected,
            "ms as a number"
        );
        assert_eq!(end_time_secs(&json!(1790204340)), expected, "seconds");
        assert_eq!(
            end_time_secs(&json!("2026-09-23T22:59:00Z")),
            expected,
            "ISO"
        );
        assert_eq!(end_time_secs(&json!(null)), None);
        assert_eq!(end_time_secs(&json!("")), None);
    }

    #[test]
    fn only_json_script_blocks_are_read() {
        let html = r#"<script id="a" type="application/json">{"x":1}</script>
                      <script id="b" src="app.js"></script>
                      <script id="c" type="application/json">{"y":2}</script>"#;
        let found = json_scripts(html);
        assert_eq!(found, vec![("a", r#"{"x":1}"#), ("c", r#"{"y":2}"#)]);
    }

    #[test]
    fn when_both_paths_fail_the_message_points_at_the_file() {
        assert!(hash_rejected_error()
            .to_string()
            .contains("ps-store-queries.json"));
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
        let quote = store
            .price("10000333", "de-DE")
            .await
            .expect("the request itself must succeed");
        assert!(
            !quote.via_page,
            "the bundled hash has been rotated — prices still work via the store page, \
             but ps-store-queries.json needs a new hash"
        );
        let price = quote.price.expect("Elden Ring has a PlayStation listing");

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
    async fn live_ps_rotated_hash_falls_back_to_the_store_page() {
        // A deliberately wrong hash: Sony refuses it exactly as it would a
        // rotated one. The price must still arrive, flagged as page-sourced.
        let mut store = PsStore::new(None).unwrap();
        store.queries.operations.insert(
            "pricingByConceptId".into(),
            Operation {
                operation_name: "metGetPricingDataByConceptId".into(),
                sha256_hash: "0".repeat(64),
            },
        );
        let quote = store.price("10000333", "de-DE").await.unwrap();
        assert!(
            quote.via_page,
            "a refused hash must be reported, not hidden"
        );
        let price = quote
            .price
            .expect("the store page still has Elden Ring's price");
        println!(
            "rotated hash -> {:.2} {} via the store page",
            price.price, price.currency
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_ps_store_page_agrees_with_the_query() {
        // The fallback is only worth having if it gives the same answer.
        let store = PsStore::new(None).unwrap();
        for concept in ["10000333", "234567"] {
            let query = store.price(concept, "de-DE").await.unwrap().price;
            let page = store.price_from_page(concept, "de-DE").await.unwrap();
            let (q, p) = (query.expect("query price"), page.expect("page price"));
            assert!(
                (q.price - p.price).abs() < 1e-9 && q.cut == p.cut && q.currency == p.currency,
                "concept {concept}: query {:.2} -{}% vs page {:.2} -{}%",
                q.price,
                q.cut,
                p.price,
                p.cut
            );
            println!(
                "concept {concept}: {:.2} {} -{}% both ways; page expiry {:?}, query expiry {:?}",
                p.price, p.currency, p.cut, p.expiry, q.expiry
            );
        }
    }
}
