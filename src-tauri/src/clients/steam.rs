use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::util::rate_limit::RateLimiter;

const API: &str = "https://api.steampowered.com";

#[derive(Debug, Deserialize)]
struct VanityEnvelope {
    response: VanityResponse,
}

#[derive(Debug, Deserialize)]
struct VanityResponse {
    steamid: Option<String>,
    success: i64,
}

#[derive(Debug, Deserialize)]
struct OwnedEnvelope {
    response: OwnedResponse,
}

#[derive(Debug, Default, Deserialize)]
struct OwnedResponse {
    game_count: Option<i64>,
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OwnedGame {
    pub appid: i64,
    #[serde(default)]
    pub name: Option<String>,
    /// Total minutes played, all time.
    #[serde(default)]
    pub playtime_forever: i64,
    /// Minutes in the last two weeks; absent unless played recently.
    #[serde(default)]
    pub playtime_2weeks: i64,
}

pub struct Steam {
    http: reqwest::Client,
    key: String,
    limiter: RateLimiter,
}

impl Steam {
    pub fn new(key: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(crate::clients::user_agent())
            .gzip(true)
            .build()
            .map_err(|e| AppError::Config(format!("could not build HTTP client: {e}")))?;
        Ok(Self { http, key, limiter: RateLimiter::per_second(2) })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        self.limiter.acquire().await;
        let res = self.http.get(format!("{API}/{path}")).query(query).send().await?;
        let status = res.status();
        if !status.is_success() {
            let hint = if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
                " — check STEAM_API_KEY in your .env"
            } else {
                ""
            };
            let body = res.text().await.unwrap_or_default();
            return Err(AppError::Api {
                service: "Steam",
                status: status.as_u16(),
                body: format!("{}{hint}", body.chars().take(200).collect::<String>()),
            });
        }
        Ok(res.json().await?)
    }

    /// Accepts a SteamID64 as-is, or resolves a vanity name
    /// (`steamcommunity.com/id/<name>`) into one.
    pub async fn resolve_steam_id(&self, input: &str) -> Result<String> {
        let trimmed = input.trim().trim_end_matches('/');
        // Tolerate a pasted profile URL in either form.
        let candidate = trimmed
            .rsplit('/')
            .next()
            .unwrap_or(trimmed)
            .trim()
            .to_string();

        // A SteamID64 is 17 digits; anything else is a vanity name.
        if candidate.len() == 17 && candidate.chars().all(|c| c.is_ascii_digit()) {
            return Ok(candidate);
        }

        let env: VanityEnvelope = self
            .get(
                "ISteamUser/ResolveVanityURL/v1/",
                &[("key", self.key.as_str()), ("vanityurl", &candidate)],
            )
            .await?;

        match (env.response.success, env.response.steamid) {
            (1, Some(id)) => Ok(id),
            _ => Err(AppError::NotFound(format!(
                "Steam has no profile called \u{201c}{candidate}\u{201d}. Use your SteamID64 \
                 or the name from your profile URL."
            ))),
        }
    }

    /// Owned games with playtime. Requires the profile's game details to be
    /// public — a private profile returns an empty list rather than an error,
    /// which is why the caller has to say so explicitly.
    pub async fn owned_games(&self, steam_id: &str) -> Result<Vec<OwnedGame>> {
        let env: OwnedEnvelope = self
            .get(
                "IPlayerService/GetOwnedGames/v1/",
                &[
                    ("key", self.key.as_str()),
                    ("steamid", steam_id),
                    ("include_appinfo", "1"),
                    ("include_played_free_games", "1"),
                ],
            )
            .await?;

        if env.response.games.is_empty() && env.response.game_count.unwrap_or(0) == 0 {
            return Err(AppError::Config(
                "Steam returned no games. Set your profile and game details to Public in \
                 Steam privacy settings, then try again."
                    .into(),
            ));
        }
        Ok(env.response.games)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Steam {
        Steam::new("dummy".into()).unwrap()
    }

    #[tokio::test]
    async fn a_steamid64_is_used_as_is() {
        let id = client().resolve_steam_id("76561197960287930").await.unwrap();
        assert_eq!(id, "76561197960287930");
    }

    #[tokio::test]
    async fn a_pasted_profile_url_is_accepted() {
        let id = client()
            .resolve_steam_id("https://steamcommunity.com/profiles/76561197960287930/")
            .await
            .unwrap();
        assert_eq!(id, "76561197960287930");
    }

    #[test]
    fn playtime_is_reported_in_minutes() {
        let g: OwnedGame = serde_json::from_str(
            r#"{"appid":427520,"name":"Factorio","playtime_forever":48726,"playtime_2weeks":97}"#,
        )
        .unwrap();
        assert_eq!(g.playtime_forever, 48726);
        assert!((g.playtime_forever as f64 / 60.0 - 812.1).abs() < 0.1);
        assert_eq!(g.playtime_2weeks, 97);
    }

    #[test]
    fn a_game_never_played_recently_has_no_two_week_field() {
        let g: OwnedGame =
            serde_json::from_str(r#"{"appid":1,"name":"X","playtime_forever":0}"#).unwrap();
        assert_eq!(g.playtime_2weeks, 0);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Hits the real Steam Web API against a public profile:
    ///   cargo test --lib live_steam -- --ignored --nocapture
    fn client() -> Steam {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".env");
        dotenvy::from_path(&root).ok();
        Steam::new(std::env::var("STEAM_API_KEY").expect("STEAM_API_KEY in .env")).unwrap()
    }

    /// A long-public Valve profile, used so the test needs nobody's own account.
    const PUBLIC_PROFILE: &str = "76561197960434622";

    #[tokio::test]
    #[ignore]
    async fn live_steam_resolves_a_vanity_name() {
        let id = client().resolve_steam_id("gabelogannewell").await.unwrap();
        assert_eq!(id.len(), 17);
        assert!(id.chars().all(|c| c.is_ascii_digit()));
        println!("vanity -> {id}");
    }

    #[tokio::test]
    #[ignore]
    async fn live_steam_owned_games_carry_playtime() {
        let games = client().owned_games(PUBLIC_PROFILE).await.unwrap();
        assert!(games.len() > 50, "expected a large public library");
        assert!(
            games.iter().any(|g| g.playtime_forever > 0),
            "no playtime at all — has the field been renamed?"
        );
        assert!(games.iter().all(|g| g.appid > 0));
        let played = games.iter().filter(|g| g.playtime_forever > 0).count();
        println!("{} games, {} played", games.len(), played);
    }

    #[tokio::test]
    #[ignore]
    async fn live_steam_appids_map_back_to_igdb_in_bulk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".env");
        dotenvy::from_path(&root).ok();
        let igdb = crate::clients::igdb::Igdb::new(
            std::env::var("IGDB_CLIENT_ID").unwrap(),
            std::env::var("IGDB_CLIENT_SECRET").unwrap(),
        )
        .unwrap();

        let games = client().owned_games(PUBLIC_PROFILE).await.unwrap();
        // Take the most-played, which are real games rather than tools.
        let mut by_time = games.clone();
        by_time.sort_by_key(|g| std::cmp::Reverse(g.playtime_forever));
        let appids: Vec<i64> = by_time.iter().take(40).map(|g| g.appid).collect();

        let matched = igdb.games_for_steam_appids(&appids).await.unwrap();
        println!("matched {}/{} of the most-played titles", matched.len(), appids.len());
        assert!(
            matched.len() * 2 > appids.len(),
            "most well-known Steam games should resolve to IGDB; got {}",
            matched.len()
        );
    }
}
