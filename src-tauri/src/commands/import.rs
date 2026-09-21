use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::clients::steam::{OwnedGame, Steam};
use crate::db::now;
use crate::error::{AppError, Result};
use crate::services::metadata;
use crate::settings;
use crate::state::AppState;

/// What a Steam account holds, before anything is imported.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamPreview {
    pub steam_id: String,
    pub total: usize,
    pub played: usize,
    pub unplayed: usize,
    /// Already in the library, so an import would only fill in playtime.
    pub already_tracked: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportOptions {
    /// Include games with recorded playtime.
    pub include_played: bool,
    /// Include games never launched — the real backlog.
    pub include_unplayed: bool,
    /// Ignore anything under this many minutes (0 = no floor).
    pub min_minutes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub considered: usize,
    /// Resolved to an IGDB game.
    pub matched: usize,
    pub added: usize,
    /// Existing entries that gained playtime they did not have.
    pub updated: usize,
    /// Owned on Steam but absent from IGDB — usually tools, demos or betas.
    pub unmatched: usize,
    pub message: Option<String>,
}

fn client(state: &AppState) -> Result<Steam> {
    let key = state.credentials().steam_api_key.ok_or_else(|| {
        AppError::Config(
            "A Steam Web API key is needed to import your library. Add STEAM_API_KEY to \
             your .env, then reload it in Settings."
                .into(),
        )
    })?;
    Steam::new(key)
}

fn selected(games: &[OwnedGame], options: &ImportOptions) -> Vec<OwnedGame> {
    games
        .iter()
        .filter(|g| {
            let played = g.playtime_forever > 0;
            let wanted = if played {
                options.include_played
            } else {
                options.include_unplayed
            };
            wanted && g.playtime_forever >= options.min_minutes
        })
        .cloned()
        .collect()
}

#[tauri::command]
pub async fn preview_steam_import(
    state: State<'_, AppState>,
    steam_id: String,
) -> Result<SteamPreview> {
    let steam = client(&state)?;
    let resolved = steam.resolve_steam_id(&steam_id).await?;
    let games = steam.owned_games(&resolved).await?;

    let played = games.iter().filter(|g| g.playtime_forever > 0).count();

    // Remember the resolved id so the import does not have to resolve again.
    state.db.with(|conn| {
        settings::patch(
            conn,
            serde_json::from_value(serde_json::json!({ "steamId": resolved }))?,
        )?;
        Ok(())
    })?;

    let appids: Vec<i64> = games.iter().map(|g| g.appid).collect();
    let already_tracked = state.db.with(|conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT 1 FROM entry e JOIN game g ON g.id = e.game_id WHERE g.steam_appid = ?1",
        )?;
        let mut count = 0;
        for appid in &appids {
            if stmt.exists([appid])? {
                count += 1;
            }
        }
        Ok(count)
    })?;

    Ok(SteamPreview {
        steam_id: resolved,
        total: games.len(),
        played,
        unplayed: games.len() - played,
        already_tracked,
    })
}

/// Import owned Steam games. Additive and safe to re-run: an existing entry is
/// never re-categorised, and hand-entered playtime is never overwritten.
#[tauri::command]
pub async fn import_steam_library(
    state: State<'_, AppState>,
    steam_id: String,
    options: ImportOptions,
) -> Result<ImportReport> {
    let steam = client(&state)?;
    let resolved = steam.resolve_steam_id(&steam_id).await?;
    let owned = steam.owned_games(&resolved).await?;
    let chosen = selected(&owned, &options);

    let mut report = ImportReport {
        considered: chosen.len(),
        matched: 0,
        added: 0,
        updated: 0,
        unmatched: 0,
        message: None,
    };

    if chosen.is_empty() {
        report.message = Some("Nothing matched those filters.".into());
        return Ok(report);
    }

    let igdb = state.igdb()?;
    let appids: Vec<i64> = chosen.iter().map(|g| g.appid).collect();
    let by_appid = igdb.games_for_steam_appids(&appids).await?;
    report.matched = by_appid.len();
    report.unmatched = chosen.len() - by_appid.len();

    // Fetch metadata only for games not already cached.
    let wanted: Vec<i64> = by_appid.values().copied().collect();
    let missing: Vec<i64> = state.db.with(|conn| {
        let mut stmt = conn.prepare_cached("SELECT 1 FROM game WHERE id = ?1")?;
        let mut out = Vec::new();
        for id in &wanted {
            if !stmt.exists([id])? {
                out.push(*id);
            }
        }
        Ok(out)
    })?;

    if !missing.is_empty() {
        let games = igdb.games_by_ids(&missing).await?;
        state.db.with(|conn| {
            for g in &games {
                metadata::upsert_game(conn, g)?;
            }
            Ok(())
        })?;
    }

    let ts = now();
    for game in &chosen {
        let Some(igdb_id) = by_appid.get(&game.appid).copied() else {
            continue;
        };

        let hours = minutes_to_hours(game.playtime_forever);
        // Recently played is the one status worth inferring; "finished" is
        // never guessed, because Steam cannot know.
        let status = if game.playtime_2weeks > 0 {
            "playing"
        } else {
            "want"
        };

        let outcome = state.db.with(|conn| {
            // Metadata may be missing if IGDB dropped the game between the two
            // calls; skip rather than violating the foreign key.
            if !conn
                .prepare_cached("SELECT 1 FROM game WHERE id = ?1")?
                .exists([igdb_id])?
            {
                return Ok("skip");
            }

            let existing: Option<(i64, Option<f64>)> = conn
                .prepare_cached("SELECT id, hours_played FROM entry WHERE game_id = ?1")?
                .query_row([igdb_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;

            match existing {
                Some((entry_id, hours_played)) => {
                    // Only fill a gap. Re-running the import must not stomp a
                    // number the user typed, or flip a game out of Finished.
                    if hours_played.is_none() && hours > 0.0 {
                        conn.prepare_cached(
                            "UPDATE entry SET hours_played = ?2, owned = 1, updated_at = ?3
                             WHERE id = ?1",
                        )?
                        .execute(params![entry_id, hours, ts])?;
                        Ok("update")
                    } else {
                        Ok("skip")
                    }
                }
                None => {
                    let top: i64 = conn
                        .prepare_cached("SELECT COALESCE(MIN(priority), 0) - 1 FROM entry")?
                        .query_row([], |r| r.get(0))?;
                    conn.prepare_cached(
                        "INSERT INTO entry (game_id, owned, status, priority, own_platform,
                                            hours_played, source, added_at, updated_at)
                         VALUES (?1, 1, ?2, ?3, 'PC', ?4, 'steam', ?5, ?5)",
                    )?
                    .execute(params![
                        igdb_id,
                        status,
                        top,
                        (hours > 0.0).then_some(hours),
                        ts
                    ])?;
                    Ok("add")
                }
            }
        })?;

        match outcome {
            "add" => report.added += 1,
            "update" => report.updated += 1,
            _ => {}
        }
    }

    if report.unmatched > 0 {
        // Name a few so the count is checkable rather than mysterious.
        let examples: Vec<&str> = chosen
            .iter()
            .filter(|g| !by_appid.contains_key(&g.appid))
            .filter_map(|g| g.name.as_deref())
            .take(3)
            .collect();

        report.message = Some(if examples.is_empty() {
            format!(
                "{} of your Steam items had no IGDB entry — usually tools, demos or betas.",
                report.unmatched
            )
        } else {
            format!(
                "{} of your Steam items had no IGDB entry (e.g. {}) — usually tools, demos \
                 or betas.",
                report.unmatched,
                examples.join(", ")
            )
        });
    }

    Ok(report)
}

/// Steam reports whole minutes; stored as hours to two decimals. Unrounded,
/// 2080 minutes became 34.6666666666667 and was shown that way.
fn minutes_to_hours(minutes: i64) -> f64 {
    (minutes as f64 / 60.0 * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playtime_is_stored_to_two_decimals() {
        // Starfield's real figure: 2080 minutes.
        assert_eq!(minutes_to_hours(2080), 34.67);
        assert_eq!(minutes_to_hours(581), 9.68);
        assert_eq!(minutes_to_hours(300), 5.0);
        assert_eq!(minutes_to_hours(1), 0.02);
        assert_eq!(minutes_to_hours(0), 0.0);
    }

    fn game(appid: i64, minutes: i64) -> OwnedGame {
        serde_json::from_str(&format!(
            r#"{{"appid":{appid},"name":"G","playtime_forever":{minutes}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn unplayed_only_selects_the_actual_backlog() {
        let games = [game(1, 0), game(2, 500), game(3, 0)];
        let picked = selected(
            &games,
            &ImportOptions {
                include_played: false,
                include_unplayed: true,
                min_minutes: 0,
            },
        );
        assert_eq!(picked.len(), 2);
        assert!(picked.iter().all(|g| g.playtime_forever == 0));
    }

    #[test]
    fn played_only_skips_games_never_launched() {
        let games = [game(1, 0), game(2, 500)];
        let picked = selected(
            &games,
            &ImportOptions {
                include_played: true,
                include_unplayed: false,
                min_minutes: 0,
            },
        );
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].appid, 2);
    }

    #[test]
    fn a_minute_floor_drops_barely_touched_games() {
        let games = [game(1, 5), game(2, 500)];
        let picked = selected(
            &games,
            &ImportOptions {
                include_played: true,
                include_unplayed: true,
                min_minutes: 60,
            },
        );
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].appid, 2);
    }

    #[test]
    fn selecting_nothing_yields_nothing_rather_than_everything() {
        let games = [game(1, 0), game(2, 500)];
        let picked = selected(&games, &ImportOptions::default());
        assert!(
            picked.is_empty(),
            "defaults must not silently import the world"
        );
    }
}
