use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

use crate::db::now;
use crate::error::Result;
use crate::settings;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total: i64,
    pub owned: i64,
    pub wishlist: i64,
    pub backlog: i64,
    pub playing: i64,
    pub finished: i64,

    pub hours_played: f64,
    pub average_rating: Option<f64>,

    pub currency: String,
    pub money_spent: f64,
    /// Sum of the cheapest current price across every wishlist game.
    pub wishlist_value: f64,
    /// What waiting has been worth: the price when each game was added, minus
    /// what was actually paid.
    pub saved_by_waiting: f64,
    /// How many purchases that figure is based on — without it the number is
    /// impossible to judge.
    pub saved_from: i64,

    pub top_genres: Vec<GenreCount>,
    pub longest_wait: Option<LongestWait>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongestWait {
    pub name: String,
    pub days: i64,
}

#[tauri::command]
pub fn get_stats(state: State<'_, AppState>) -> Result<Stats> {
    state.db.with(|conn| {
        let settings = settings::read(conn)?;

        let count = |sql: &str| -> Result<i64> {
            Ok(conn.prepare_cached(sql)?.query_row([], |r| r.get(0))?)
        };

        let total = count("SELECT COUNT(*) FROM entry")?;
        let owned = count("SELECT COUNT(*) FROM entry WHERE owned = 1")?;
        let wishlist = count("SELECT COUNT(*) FROM entry WHERE owned = 0 AND status = 'want'")?;
        let backlog = count("SELECT COUNT(*) FROM entry WHERE owned = 1 AND status = 'want'")?;
        let playing = count("SELECT COUNT(*) FROM entry WHERE status = 'playing'")?;
        let finished = count("SELECT COUNT(*) FROM entry WHERE status IN ('finished','dropped')")?;

        let hours_played: f64 = conn
            .prepare_cached("SELECT COALESCE(SUM(hours_played), 0) FROM entry")?
            .query_row([], |r| r.get(0))?;

        let average_rating: Option<f64> = conn
            .prepare_cached("SELECT AVG(my_rating) FROM entry WHERE my_rating IS NOT NULL")?
            .query_row([], |r| r.get(0))?;

        let money_spent: f64 = conn
            .prepare_cached("SELECT COALESCE(SUM(purchase_price), 0) FROM entry")?
            .query_row([], |r| r.get(0))?;

        // Cheapest current offer per wishlist game, summed.
        let wishlist_value: f64 = conn
            .prepare_cached(
                "SELECT COALESCE(SUM(cheapest), 0) FROM (
                   SELECT MIN(s.price) AS cheapest
                     FROM entry e
                     JOIN price_snapshot s ON s.game_id = e.game_id AND s.country = ?1
                    WHERE e.owned = 0 AND e.status = 'want'
                    GROUP BY e.game_id)",
            )?
            .query_row([&settings.country], |r| r.get(0))?;

        // Only purchases where both numbers exist can contribute.
        let (saved_by_waiting, saved_from): (f64, i64) = conn
            .prepare_cached(
                "SELECT COALESCE(SUM(price_at_add - purchase_price), 0), COUNT(*)
                   FROM entry
                  WHERE price_at_add IS NOT NULL AND purchase_price IS NOT NULL",
            )?
            .query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;

        let mut stmt = conn.prepare_cached(
            "SELECT g.name, COUNT(*) c FROM genre g
               JOIN game_genre gg ON gg.genre_id = g.id
               JOIN entry e ON e.game_id = gg.game_id
              GROUP BY g.id ORDER BY c DESC, g.name LIMIT 8",
        )?;
        let top_genres = stmt
            .query_map([], |r| {
                Ok(GenreCount {
                    name: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let longest_wait = conn
            .prepare_cached(
                "SELECT g.name, e.added_at FROM entry e JOIN game g ON g.id = e.game_id
                  WHERE e.owned = 0 AND e.status = 'want'
                  ORDER BY e.added_at ASC LIMIT 1",
            )?
            .query_row([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .optional()?
            .map(|(name, added)| LongestWait {
                name,
                days: (now() - added) / 86_400,
            });

        Ok(Stats {
            total,
            owned,
            wishlist,
            backlog,
            playing,
            finished,
            hours_played,
            average_rating,
            currency: settings.currency,
            money_spent,
            wishlist_value,
            saved_by_waiting,
            saved_from,
            top_genres,
            longest_wait,
        })
    })
}
