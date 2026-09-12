use tauri::State;

use crate::db::models::{GameDetail, SearchResult};
use crate::error::{AppError, Result};
use crate::services::metadata;
use crate::state::AppState;

/// IGDB's relevance ordering alone puts "Elden Ring Nightreign" above "Elden
/// Ring" and "Hollow Knight Silksong" near the original. Blend its ordering
/// with how many people have actually rated the game, and reward a literal
/// title match, so the game someone typed the name of comes first.
fn score(position: usize, name: &str, term: &str, rating_count: i64) -> f64 {
    let name = name.trim().to_lowercase();
    let term = term.trim().to_lowercase();

    let exact = if name == term { 3.0 } else { 0.0 };
    let prefix = if exact == 0.0 && name.starts_with(&term) { 1.5 } else { 0.0 };
    // log so a 5000-vote classic outranks a 500-vote sequel without a
    // 100,000-vote outlier flattening everything below it.
    let popularity = ((1 + rating_count.max(0)) as f64).ln();
    let relevance = 0.5 / (1.0 + position as f64);

    popularity + exact + prefix + relevance
}

fn release_year(unix: Option<i64>) -> Option<i32> {
    let ts = unix?;
    // Civil-from-days, valid for any date SQLite will hold. Avoids pulling in
    // chrono for one conversion.
    let days = ts.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let y = yoe + era * 400 + if mp >= 10 { 1 } else { 0 };
    Some(y as i32)
}

#[tauri::command]
pub async fn search_games(state: State<'_, AppState>, query: String) -> Result<Vec<SearchResult>> {
    let term = query.trim().to_string();
    if term.len() < 2 {
        return Ok(Vec::new());
    }

    let games = state.igdb()?.search(&term, 30).await?;

    let mut results: Vec<SearchResult> = games
        .iter()
        .enumerate()
        .map(|(_, g)| SearchResult {
            igdb_id: g.id,
            name: g.name.clone(),
            cover_image_id: g.cover_image_id(),
            release_year: release_year(g.first_release_date),
            platforms: g
                .platforms
                .iter()
                .filter_map(|p| p.abbreviation.clone().or_else(|| Some(p.name.clone())))
                .take(6)
                .collect(),
            rating_count: g.total_rating_count.unwrap_or(0),
            in_library: false,
        })
        .collect();

    let ranked: Vec<f64> = games
        .iter()
        .enumerate()
        .map(|(i, g)| score(i, &g.name, &term, g.total_rating_count.unwrap_or(0)))
        .collect();

    let mut order: Vec<usize> = (0..results.len()).collect();
    order.sort_by(|a, b| ranked[*b].partial_cmp(&ranked[*a]).unwrap_or(std::cmp::Ordering::Equal));
    let mut reordered: Vec<SearchResult> = order.into_iter().map(|i| results[i].clone()).collect();
    std::mem::swap(&mut results, &mut reordered);
    results.truncate(20);

    // Flag the ones already tracked so the UI can show "In library".
    state.db.with(|conn| {
        let mut stmt = conn.prepare_cached("SELECT 1 FROM entry WHERE game_id = ?1")?;
        for r in results.iter_mut() {
            r.in_library = stmt.exists([r.igdb_id])?;
        }
        Ok(())
    })?;

    Ok(results)
}

/// Full metadata for one game, served from the local cache and refetched from
/// IGDB when absent or when `refresh` is set.
#[tauri::command]
pub async fn get_game(state: State<'_, AppState>, igdb_id: i64, refresh: bool) -> Result<GameDetail> {
    if !refresh {
        if let Some(detail) = state.db.with(|conn| metadata::read_detail(conn, igdb_id))? {
            return Ok(detail);
        }
    }

    let game = state
        .igdb()?
        .game(igdb_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("IGDB has no game {igdb_id}")))?;

    state.db.with(|conn| metadata::upsert_game(conn, &game))?;

    state
        .db
        .with(|conn| metadata::read_detail(conn, igdb_id))?
        .ok_or_else(|| AppError::NotFound(format!("game {igdb_id} vanished after saving")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(term: &str, candidates: &[(&str, i64)]) -> Vec<String> {
        let mut scored: Vec<(f64, String)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (name, votes))| (score(i, name, term, *votes), (*name).to_string()))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        scored.into_iter().map(|(_, n)| n).collect()
    }

    #[test]
    fn the_game_you_named_outranks_its_more_recent_sequel() {
        // IGDB returns Nightreign first for this query.
        let ranked = rank("elden ring", &[("Elden Ring Nightreign", 106), ("Elden Ring", 2284)]);
        assert_eq!(ranked[0], "Elden Ring");
    }

    #[test]
    fn an_exact_title_wins_over_a_more_popular_relative() {
        let ranked = rank(
            "hollow knight",
            &[("Hollow Knight: Silksong", 5000), ("Hollow Knight", 2237)],
        );
        assert_eq!(ranked[0], "Hollow Knight");
    }

    #[test]
    fn obscure_shovelware_never_outranks_the_real_thing() {
        let ranked = rank(
            "witcher 3",
            &[
                ("The Witcher 3: Wild Hunt - Collector's Edition", 0),
                ("The Witcher 3: Wild Hunt", 5476),
            ],
        );
        assert_eq!(ranked[0], "The Witcher 3: Wild Hunt");
    }

    #[test]
    fn release_year_matches_known_release_dates() {
        assert_eq!(release_year(Some(1487894400)), Some(2017)); // Hollow Knight
        assert_eq!(release_year(Some(1645747200)), Some(2022)); // Elden Ring
        assert_eq!(release_year(Some(0)), Some(1970));
        assert_eq!(release_year(None), None);
    }
}
