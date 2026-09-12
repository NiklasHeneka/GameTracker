use std::time::Duration;

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::{AppError, Result};
use crate::util::rate_limit::RateLimiter;

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const API: &str = "https://api.igdb.com/v4";

/// Game types worth showing in search. Deliberately excludes mods (5), DLC (1),
/// expansions (2), bundles (3) and packs (13) — searching "hollow knight"
/// otherwise surfaces an unofficial Vita mod above the real game.
const SEARCHABLE_TYPES: &str = "(0,4,8,9,10)";

const GAME_FIELDS: &str = "\
fields name,slug,summary,first_release_date,total_rating,total_rating_count,game_type,\
cover.image_id,artworks.image_id,genres.name,\
platforms.id,platforms.name,platforms.abbreviation,platforms.platform_family,\
external_games.uid,external_games.external_game_source,\
involved_companies.developer,involved_companies.publisher,involved_companies.company.name;";

/// IGDB's `external_game_source` id for Steam. The `category` field this
/// replaced is gone — and IGDB silently drops unknown fields rather than
/// erroring, so a stale name here would fail as "no Steam id" forever.
const SOURCE_STEAM: i64 = 1;

/// `external_game_source` for the PlayStation Store. The uid is the numeric
/// *concept* id the PS Store GraphQL API takes — concept ids are global, and
/// the region comes from a locale header, so the "US" in IGDB's label for this
/// source does not restrict it.
const SOURCE_PLAYSTATION: i64 = 36;

// ── Wire types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Image {
    pub image_id: String,
}

#[derive(Debug, Deserialize)]
pub struct NamedRef {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct IgdbPlatform {
    pub id: i64,
    pub name: String,
    pub abbreviation: Option<String>,
    pub platform_family: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ExternalGame {
    pub uid: Option<String>,
    pub external_game_source: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct InvolvedCompany {
    pub company: Option<NamedRef>,
    #[serde(default)]
    pub developer: bool,
    #[serde(default)]
    pub publisher: bool,
}

#[derive(Debug, Deserialize)]
struct ExternalGameRow {
    game: Option<i64>,
    uid: String,
}

#[derive(Debug, Deserialize)]
pub struct IgdbGame {
    pub id: i64,
    pub name: String,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub first_release_date: Option<i64>,
    pub total_rating: Option<f64>,
    pub total_rating_count: Option<i64>,
    pub cover: Option<Image>,
    #[serde(default)]
    pub artworks: Vec<Image>,
    #[serde(default)]
    pub genres: Vec<NamedRef>,
    #[serde(default)]
    pub platforms: Vec<IgdbPlatform>,
    #[serde(default)]
    pub external_games: Vec<ExternalGame>,
    #[serde(default)]
    pub involved_companies: Vec<InvolvedCompany>,
}

impl IgdbGame {
    /// The Steam appid, which is the join key to every price source.
    pub fn steam_appid(&self) -> Option<i64> {
        self.external_games
            .iter()
            .find(|e| e.external_game_source == Some(SOURCE_STEAM))
            .and_then(|e| e.uid.as_deref())
            .and_then(|uid| uid.parse().ok())
    }

    /// The PlayStation Store concept id, if IGDB knows one.
    pub fn psn_concept_id(&self) -> Option<String> {
        self.external_games
            .iter()
            .find(|e| e.external_game_source == Some(SOURCE_PLAYSTATION))
            .and_then(|e| e.uid.clone())
            .filter(|uid| uid.chars().all(|c| c.is_ascii_digit()) && !uid.is_empty())
    }

    pub fn company(&self, developer: bool) -> Option<String> {
        self.involved_companies
            .iter()
            .find(|c| if developer { c.developer } else { c.publisher })
            .and_then(|c| c.company.as_ref())
            .map(|c| c.name.clone())
    }

    pub fn cover_image_id(&self) -> Option<String> {
        self.cover.as_ref().map(|c| c.image_id.clone())
    }

    pub fn artwork_image_id(&self) -> Option<String> {
        self.artworks.first().map(|a| a.image_id.clone())
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct CachedToken {
    value: String,
    expires_at: std::time::Instant,
}

// ── Client ───────────────────────────────────────────────────────────────

pub struct Igdb {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    token: RwLock<Option<CachedToken>>,
    limiter: RateLimiter,
}

impl Igdb {
    pub fn new(client_id: String, client_secret: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(crate::clients::user_agent())
            .gzip(true)
            .build()
            .map_err(|e| AppError::Config(format!("could not build HTTP client: {e}")))?;

        Ok(Self {
            http,
            client_id,
            client_secret,
            token: RwLock::new(None),
            limiter: RateLimiter::per_second(4),
        })
    }

    /// App access tokens last ~60 days. Kept in memory only: re-fetching once
    /// per launch is a single request, and it avoids persisting a bearer token
    /// to disk alongside the database.
    async fn access_token(&self) -> Result<String> {
        if let Some(t) = self.token.read().await.as_ref() {
            if t.expires_at > std::time::Instant::now() {
                return Ok(t.value.clone());
            }
        }

        let mut slot = self.token.write().await;
        // Another task may have refreshed while we waited for the write lock.
        if let Some(t) = slot.as_ref() {
            if t.expires_at > std::time::Instant::now() {
                return Ok(t.value.clone());
            }
        }

        let res = self
            .http
            .post(TOKEN_URL)
            .query(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(AppError::Config(format!(
                "IGDB sign-in failed ({status}). Check IGDB_CLIENT_ID and \
                 IGDB_CLIENT_SECRET in your .env. {body}"
            )));
        }

        let token: TokenResponse = res.json().await?;
        // Renew an hour early so a long-running session never races expiry.
        let lifetime = Duration::from_secs(token.expires_in.saturating_sub(3600));
        log::info!("IGDB token acquired, valid for {} days", lifetime.as_secs() / 86400);

        *slot = Some(CachedToken {
            value: token.access_token.clone(),
            expires_at: std::time::Instant::now() + lifetime,
        });

        Ok(token.access_token)
    }

    async fn query<T: serde::de::DeserializeOwned>(&self, endpoint: &str, body: String) -> Result<T> {
        self.limiter.acquire().await;

        let send = |token: String, body: String| {
            self.http
                .post(format!("{API}/{endpoint}"))
                .header("Client-ID", &self.client_id)
                .bearer_auth(token)
                .header("Content-Type", "text/plain")
                .body(body)
                .send()
        };

        let mut res = send(self.access_token().await?, body.clone()).await?;

        // A revoked or expired token reads as 401; drop the cache and retry once.
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            log::warn!("IGDB rejected the token; re-authenticating");
            *self.token.write().await = None;
            self.limiter.acquire().await;
            res = send(self.access_token().await?, body).await?;
        }

        let status = res.status();
        if !status.is_success() {
            let detail = res.text().await.unwrap_or_default();
            return Err(AppError::Api {
                service: "IGDB",
                status: status.as_u16(),
                body: detail.chars().take(400).collect(),
            });
        }

        Ok(res.json().await?)
    }

    pub async fn search(&self, term: &str, limit: u32) -> Result<Vec<IgdbGame>> {
        let escaped = term.replace('"', "");
        self.query(
            "games",
            format!(
                "{GAME_FIELDS} search \"{escaped}\"; \
                 where game_type = {SEARCHABLE_TYPES} & version_parent = null; \
                 limit {limit};"
            ),
        )
        .await
    }

    /// Map Steam appids back to IGDB game ids, in batches.
    ///
    /// IGDB caps a response at 500 rows, and the Apicalypse `where … = (…)`
    /// list has to stay a reasonable length, so requests are chunked.
    pub async fn games_for_steam_appids(
        &self,
        appids: &[i64],
    ) -> Result<std::collections::HashMap<i64, i64>> {
        let mut found = std::collections::HashMap::new();

        for chunk in appids.chunks(200) {
            let list = chunk
                .iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",");

            let rows: Vec<ExternalGameRow> = self
                .query(
                    "external_games",
                    format!(
                        "fields game,uid; \
                         where external_game_source = {SOURCE_STEAM} & uid = ({list}); \
                         limit 500;"
                    ),
                )
                .await?;

            for row in rows {
                if let (Some(game), Ok(appid)) = (row.game, row.uid.parse::<i64>()) {
                    found.entry(appid).or_insert(game);
                }
            }
        }

        Ok(found)
    }

    /// Full metadata for many games at once, so an import does not make one
    /// request per title.
    pub async fn games_by_ids(&self, ids: &[i64]) -> Result<Vec<IgdbGame>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(200) {
            let list = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            let mut games: Vec<IgdbGame> = self
                .query("games", format!("{GAME_FIELDS} where id = ({list}); limit 500;"))
                .await?;
            out.append(&mut games);
        }
        Ok(out)
    }

    pub async fn game(&self, igdb_id: i64) -> Result<Option<IgdbGame>> {
        let mut games: Vec<IgdbGame> = self
            .query("games", format!("{GAME_FIELDS} where id = {igdb_id}; limit 1;"))
            .await?;
        Ok(games.pop())
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the real IGDB API. Ignored by default; run with:
    ///   cargo test --lib live -- --ignored --nocapture
    /// Guards against IGDB renaming fields out from under us — it drops
    /// unknown field names silently rather than erroring, so a rename shows up
    /// as permanently-missing data rather than a failed request.
    fn client() -> Igdb {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".env");
        dotenvy::from_path(&root).ok();
        Igdb::new(
            std::env::var("IGDB_CLIENT_ID").expect("IGDB_CLIENT_ID in .env"),
            std::env::var("IGDB_CLIENT_SECRET").expect("IGDB_CLIENT_SECRET in .env"),
        )
        .unwrap()
    }

    #[tokio::test]
    #[ignore]
    async fn live_search_returns_populated_games() {
        let games = client().search("hollow knight", 10).await.unwrap();
        assert!(!games.is_empty(), "search returned nothing");

        let hk = games
            .iter()
            .find(|g| g.name == "Hollow Knight")
            .expect("Hollow Knight in results");

        // Each of these is a field IGDB could rename; assert they arrive.
        assert!(hk.cover_image_id().is_some(), "cover.image_id missing");
        assert!(!hk.genres.is_empty(), "genres missing");
        assert!(!hk.platforms.is_empty(), "platforms missing");
        assert!(hk.total_rating_count.unwrap_or(0) > 100, "total_rating_count missing");
        assert!(hk.first_release_date.is_some(), "first_release_date missing");

        // The mod filter must keep unofficial ports out.
        assert!(
            !games.iter().any(|g| g.summary.as_deref().unwrap_or("").contains("Unofficial port")),
            "game_type filter let a mod through"
        );
        println!("search ok — {} results, top: {}", games.len(), games[0].name);
    }

    #[tokio::test]
    #[ignore]
    async fn live_game_carries_steam_appid_and_credits() {
        let game = client().game(14593).await.unwrap().expect("Hollow Knight by id");
        assert_eq!(game.name, "Hollow Knight");
        assert_eq!(
            game.steam_appid(),
            Some(367520),
            "external_game_source lookup broke — is Steam still source 1?"
        );
        assert!(game.company(true).is_some(), "developer credit missing");
        assert!(game.artwork_image_id().is_some(), "artworks missing");
        println!(
            "detail ok — appid {:?}, dev {:?}, platforms {}",
            game.steam_appid(),
            game.company(true),
            game.platforms.len()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_rate_limiter_keeps_us_under_igdbs_ceiling() {
        let igdb = client();
        let start = std::time::Instant::now();
        for _ in 0..8 {
            igdb.game(14593).await.unwrap();
        }
        // 8 requests at 4/s cannot legitimately finish faster than ~1.75s.
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() >= 1500, "limiter let a burst through in {elapsed:?}");
        println!("8 sequential requests took {elapsed:?} (no 429s)");
    }
}
