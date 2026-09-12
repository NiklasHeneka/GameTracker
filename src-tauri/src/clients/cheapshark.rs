use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::{AppError, Result};
use crate::util::rate_limit::RateLimiter;

const API: &str = "https://www.cheapshark.com/api/1.0";

/// CheapShark quotes **USD only** — it has no country or currency parameter.
/// Anything shown from this source must be labelled as such rather than
/// rendered in the user's configured currency.
pub const CURRENCY: &str = "USD";

#[derive(Debug, Deserialize)]
pub struct Store {
    #[serde(rename = "storeID")]
    pub store_id: String,
    #[serde(rename = "storeName")]
    pub store_name: String,
    #[serde(rename = "isActive")]
    pub is_active: i64,
}

#[derive(Debug, Deserialize)]
pub struct GameMatch {
    #[serde(rename = "gameID")]
    pub game_id: String,
}

#[derive(Debug, Deserialize)]
pub struct Deal {
    #[serde(rename = "storeID")]
    pub store_id: String,
    pub price: String,
    #[serde(rename = "retailPrice")]
    pub retail_price: String,
    pub savings: String,
    #[serde(rename = "dealID")]
    pub deal_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CheapestEver {
    pub price: String,
    pub date: i64,
}

#[derive(Debug, Deserialize)]
pub struct GameDetail {
    #[serde(rename = "cheapestPriceEver")]
    pub cheapest_price_ever: Option<CheapestEver>,
    #[serde(default)]
    pub deals: Vec<Deal>,
}

/// Free, keyless PC price source. Used when no IsThereAnyDeal key is
/// configured, so the app shows prices out of the box.
pub struct CheapShark {
    http: reqwest::Client,
    limiter: RateLimiter,
    /// storeID → display name, fetched once and cached for the session.
    stores: RwLock<Option<HashMap<String, String>>>,
}

impl CheapShark {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            // CheapShark rejects a missing or generic User-Agent outright.
            .user_agent(crate::clients::user_agent())
            .gzip(true)
            .build()
            .map_err(|e| AppError::Config(format!("could not build HTTP client: {e}")))?;

        Ok(Self {
            http,
            limiter: RateLimiter::per_second(3),
            stores: RwLock::new(None),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        self.limiter.acquire().await;
        let res = self
            .http
            .get(format!("{API}/{path}"))
            .query(query)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(AppError::Api {
                service: "CheapShark",
                status: status.as_u16(),
                body: body.chars().take(300).collect(),
            });
        }

        let text = res.text().await?;
        // A missing/generic User-Agent yields a 200 with an error object, not a
        // failure status — surface that as an error rather than a parse fault.
        if text.contains("\"error\"") && text.len() < 300 {
            return Err(AppError::Api {
                service: "CheapShark",
                status: 200,
                body: text,
            });
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn store_names(&self) -> Result<HashMap<String, String>> {
        if let Some(cached) = self.stores.read().await.as_ref() {
            return Ok(cached.clone());
        }
        let stores: Vec<Store> = self.get("stores", &[]).await?;
        let map: HashMap<String, String> = stores
            .into_iter()
            .filter(|s| s.is_active == 1)
            .map(|s| (s.store_id, s.store_name))
            .collect();
        *self.stores.write().await = Some(map.clone());
        Ok(map)
    }

    /// Resolve CheapShark's own game id from a Steam appid — far more reliable
    /// than matching on title, which confuses editions and remasters.
    pub async fn id_for_steam_appid(&self, appid: i64) -> Result<Option<String>> {
        let matches: Vec<GameMatch> = self
            .get("games", &[("steamAppID", &appid.to_string())])
            .await?;
        Ok(matches.into_iter().next().map(|m| m.game_id))
    }

    pub async fn game(&self, cheapshark_id: &str) -> Result<GameDetail> {
        self.get("games", &[("id", cheapshark_id)]).await
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the real CheapShark API:
    ///   cargo test --lib live_cheapshark -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_cheapshark_full_pipeline() {
        let cs = CheapShark::new().unwrap();

        let stores = cs.store_names().await.unwrap();
        assert!(stores.len() > 5, "expected several active stores");
        assert_eq!(stores.get("1").map(String::as_str), Some("Steam"));

        let id = cs
            .id_for_steam_appid(367520)
            .await
            .unwrap()
            .expect("Hollow Knight resolves from its Steam appid");

        let detail = cs.game(&id).await.unwrap();
        assert!(!detail.deals.is_empty(), "no deals returned");
        let ever = detail.cheapest_price_ever.expect("a historical low");
        assert!(ever.price.parse::<f64>().unwrap() > 0.0);
        println!(
            "{} deals, cheapest ever {} USD on {}",
            detail.deals.len(),
            ever.price,
            ever.date
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_cheapshark_requires_a_descriptive_user_agent() {
        // CheapShark answers 200 with an error object when the UA is generic,
        // which is exactly the trap the client guards against.
        let naked = reqwest::Client::builder().user_agent("curl/8.0").build().unwrap();
        let body = naked
            .get("https://www.cheapshark.com/api/1.0/stores")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        println!("generic UA -> {}", &body[..body.len().min(120)]);
        assert!(body.contains("error"), "CheapShark stopped rejecting generic agents");
    }
}
