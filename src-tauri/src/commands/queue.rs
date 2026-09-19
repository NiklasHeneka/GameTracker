//! The Play Next list: an ordered queue of what to play, drawn from games
//! already in the library.
//!
//! Kept separate from the board on purpose. `entry.priority` orders the four
//! columns; this list has its own `position` so reordering one never disturbs
//! the other. A queued game may be unowned, owned-and-unplayed, or finished
//! and worth revisiting — the queue answers "what next?", not "do I own it?".

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

use crate::commands::library::read_entry;
use crate::db::models::LibraryEntry;
use crate::db::now;
use crate::db::price_models::{PriceRow, StoreSaleOutlook};
use crate::error::{AppError, Result};
use crate::services::{pricing, sale_calendar};
use crate::settings::{self, Settings};
use crate::state::AppState;

/// One store's answer for one game: what it costs there now, and when that
/// store's next storewide sale is.
///
/// The two halves come from different places — `price_snapshot` for the offer,
/// the sale calendar for the outlook — and the row shows one store at a time,
/// so they are merged here rather than in the interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStore {
    pub shop: String,
    /// `None` when the store carries no listing for this game.
    pub offer: Option<PriceRow>,
    /// `None` when the calendar knows of no upcoming sale at this store.
    pub outlook: Option<StoreSaleOutlook>,
}

/// One row of the list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueRow {
    pub entry: LibraryEntry,
    /// Which store's price the row shows. `None` means "whichever is cheapest",
    /// which is `stores` in the order it arrives.
    pub preferred_shop: Option<String>,
    /// Cheapest offer first, then stores that only have a sale to report.
    /// **Always empty for a game already owned**, where the price is a question
    /// already answered.
    pub stores: Vec<QueueStore>,
    pub added_at: i64,
    pub fetched_at: Option<i64>,
    pub stale: bool,
}

/// Merge the current offers and the sale calendar into one entry per store.
fn stores_for(
    conn: &Connection,
    cal: &sale_calendar::Calendar,
    game_id: i64,
    country: &str,
    shops: &[String],
) -> Result<Vec<QueueStore>> {
    let offers = pricing::offers(conn, game_id, country, shops)?;
    let outlooks = sale_calendar::outlook_with(cal, conn, game_id, country, shops)?;

    // Offers come back cheapest first, which is the order the picker wants.
    let mut out: Vec<QueueStore> = offers
        .into_iter()
        .map(|offer| QueueStore {
            outlook: outlooks.iter().find(|o| o.shop == offer.shop).cloned(),
            shop: offer.shop.clone(),
            offer: Some(offer),
        })
        .collect();

    // A store with no listing still has something to say — its next sale — and
    // dropping it would hide the answer to "should I wait?".
    for outlook in outlooks {
        if out.iter().any(|s| s.shop == outlook.shop) {
            continue;
        }
        out.push(QueueStore {
            shop: outlook.shop.clone(),
            offer: None,
            outlook: Some(outlook),
        });
    }

    Ok(out)
}

/// Build one row: the entry, plus — only when it is not owned — the per-store
/// prices and how fresh they are.
fn build_row(
    conn: &Connection,
    cal: &sale_calendar::Calendar,
    settings: &Settings,
    entry_id: i64,
    preferred_shop: Option<String>,
    added_at: i64,
) -> Result<QueueRow> {
    let entry = read_entry(conn, entry_id)?;
    let game_id = entry.game.igdb_id;

    // Owned games show ownership instead of a price, so there is nothing to
    // assemble — which also keeps this cheap for what is usually most of the list.
    let (stores, fetched) = if entry.owned {
        (Vec::new(), None)
    } else {
        (
            stores_for(
                conn,
                cal,
                game_id,
                &settings.country,
                &settings.enabled_shops,
            )?,
            pricing::fetched_at(conn, game_id, &settings.country)?,
        )
    };

    Ok(QueueRow {
        // An owned row shows no price, so it can never be out of date.
        stale: !entry.owned
            && fetched
                .map(|t| now() - t > pricing::FRESH_FOR_SECS)
                .unwrap_or(true),
        entry,
        preferred_shop,
        stores,
        added_at,
        fetched_at: fetched,
    })
}

/// The queue in display order, newest positions last.
///
/// Reads the database and nothing else. Twenty rows refreshed on every visit
/// would be twenty ITAD lookups, twenty five-year history pulls and twenty
/// PlayStation calls — slow, and a good way to spend the request budget on
/// nothing. `stale` tells the page when to offer a refresh instead.
pub fn list(conn: &Connection, config_dir: Option<&Path>) -> Result<Vec<QueueRow>> {
    let settings = settings::read(conn)?;
    let queued: Vec<(i64, Option<String>, i64)> = {
        let mut stmt = conn.prepare_cached(
            "SELECT entry_id, preferred_shop, added_at FROM queue ORDER BY position ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.collect::<std::result::Result<_, _>>()?
    };

    // Loaded once for the whole page rather than once per row: `outlook` would
    // otherwise re-read and re-parse the calendar file for every game.
    let cal = sale_calendar::load(config_dir);

    let mut out = Vec::new();
    for (entry_id, preferred_shop, added_at) in queued {
        match build_row(conn, &cal, &settings, entry_id, preferred_shop, added_at) {
            Ok(row) => out.push(row),
            // The foreign key makes this unreachable in practice, but a single
            // bad row must not blank the whole page.
            Err(e) => log::warn!("queued entry {entry_id} could not be read: {e}"),
        }
    }
    Ok(out)
}

fn read_row(conn: &Connection, entry_id: i64, config_dir: Option<&Path>) -> Result<QueueRow> {
    let found = conn
        .prepare_cached("SELECT preferred_shop, added_at FROM queue WHERE entry_id = ?1")?
        .query_row([entry_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
        })
        .optional()?;

    let Some((preferred_shop, added_at)) = found else {
        return Err(AppError::NotFound(format!(
            "queue row for entry {entry_id}"
        )));
    };

    let settings = settings::read(conn)?;
    let cal = sale_calendar::load(config_dir);
    build_row(conn, &cal, &settings, entry_id, preferred_shop, added_at)
}

/// Append to the end of the list.
///
/// The end, not the top: the first slot means "this is what I play next" and
/// should not be taken by whatever was added last. (`add_entry` does the
/// opposite for the board, where a new game genuinely is the freshest thing.)
///
/// Idempotent — queueing something already queued returns the existing row
/// rather than moving it, so a double click cannot reshuffle the list.
pub fn add(conn: &Connection, entry_id: i64, config_dir: Option<&Path>) -> Result<QueueRow> {
    let exists = conn
        .prepare_cached("SELECT 1 FROM entry WHERE id = ?1")?
        .exists([entry_id])?;
    if !exists {
        return Err(AppError::NotFound(format!("library entry {entry_id}")));
    }

    conn.prepare_cached(
        "INSERT INTO queue (entry_id, position, added_at)
         VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM queue), ?2)
         ON CONFLICT (entry_id) DO NOTHING",
    )?
    .execute(params![entry_id, now()])?;

    read_row(conn, entry_id, config_dir)
}

/// Remove from the list. The library entry itself is untouched — that is the
/// whole point of the queue being a separate table.
///
/// Positions are left with a gap. Order comes from `ORDER BY position`, which
/// does not care, and the next drag rewrites them all anyway.
pub fn remove(conn: &Connection, entry_id: i64) -> Result<()> {
    let affected = conn
        .prepare_cached("DELETE FROM queue WHERE entry_id = ?1")?
        .execute([entry_id])?;
    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "queue row for entry {entry_id}"
        )));
    }
    Ok(())
}

/// Remember which store's price this row shows. `None` goes back to
/// "whichever is cheapest", which is what an unset row does.
pub fn set_shop(conn: &Connection, entry_id: i64, shop: Option<String>) -> Result<()> {
    let affected = conn
        .prepare_cached("UPDATE queue SET preferred_shop = ?2 WHERE entry_id = ?1")?
        .execute(params![entry_id, shop])?;
    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "queue row for entry {entry_id}"
        )));
    }
    Ok(())
}

/// Persist a drag-reordering; `ids` is the new order, front to back.
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached("UPDATE queue SET position = ?2 WHERE entry_id = ?1")?;
        for (position, id) in ids.iter().enumerate() {
            stmt.execute(params![id, position as i64])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[tauri::command]
pub fn list_queue(state: State<'_, AppState>) -> Result<Vec<QueueRow>> {
    let dir = state.env_path.parent().map(|p| p.to_path_buf());
    state.db.with(|conn| list(conn, dir.as_deref()))
}

#[tauri::command]
pub fn enqueue(state: State<'_, AppState>, entry_id: i64) -> Result<QueueRow> {
    let dir = state.env_path.parent().map(|p| p.to_path_buf());
    state.db.with(|conn| add(conn, entry_id, dir.as_deref()))
}

#[tauri::command]
pub fn dequeue(state: State<'_, AppState>, entry_id: i64) -> Result<()> {
    state.db.with(|conn| remove(conn, entry_id))
}

#[tauri::command]
pub fn reorder_queue(state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    state.db.with(|conn| reorder(conn, &ids))
}

#[tauri::command]
pub fn set_queue_shop(
    state: State<'_, AppState>,
    entry_id: i64,
    shop: Option<String>,
) -> Result<QueueRow> {
    let dir = state.env_path.parent().map(|p| p.to_path_buf());
    state.db.with(|conn| {
        set_shop(conn, entry_id, shop)?;
        read_row(conn, entry_id, dir.as_deref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    /// A game plus its library entry, the minimum a queue row needs.
    fn entry(conn: &Connection, igdb_id: i64, name: &str) -> i64 {
        owned_entry(conn, igdb_id, name, false)
    }

    fn owned_entry(conn: &Connection, igdb_id: i64, name: &str, owned: bool) -> i64 {
        conn.execute(
            "INSERT INTO game (id, name, metadata_fetched) VALUES (?1, ?2, 0)",
            params![igdb_id, name],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entry (game_id, owned, status, priority, added_at, updated_at)
             VALUES (?1, ?2, 'want', 0, 0, 0)",
            params![igdb_id, owned as i64],
        )
        .unwrap();
        conn.query_row("SELECT id FROM entry WHERE game_id = ?1", [igdb_id], |r| {
            r.get(0)
        })
        .unwrap()
    }

    /// A current offer at one shop, in the default country.
    fn offer(conn: &Connection, game_id: i64, shop: &str, price: f64) {
        conn.execute(
            "INSERT INTO price_snapshot
               (game_id, shop, country, platform_family, currency, price, cut, source, fetched_at)
             VALUES (?1, ?2, 'DE', 'pc', 'EUR', ?3, 0, 'itad', ?4)",
            params![game_id, shop, price, now()],
        )
        .unwrap();
    }

    fn order(conn: &Connection) -> Vec<i64> {
        list(conn, None)
            .unwrap()
            .into_iter()
            .map(|r| r.entry.id)
            .collect()
    }

    #[test]
    fn new_rows_go_to_the_back() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        let c = entry(&conn, 3, "C");

        add(&conn, a, None).unwrap();
        add(&conn, b, None).unwrap();
        add(&conn, c, None).unwrap();

        assert_eq!(order(&conn), vec![a, b, c]);
    }

    #[test]
    fn queueing_twice_does_not_move_the_row() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        add(&conn, a, None).unwrap();
        add(&conn, b, None).unwrap();

        // A double click must not shunt A to the back of the list.
        add(&conn, a, None).unwrap();

        assert_eq!(order(&conn), vec![a, b]);
    }

    #[test]
    fn a_gap_left_by_a_removal_does_not_collide_with_the_next_add() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        let c = entry(&conn, 3, "C");
        add(&conn, a, None).unwrap();
        add(&conn, b, None).unwrap();

        // Removing the last row leaves MAX(position) = 0, so C must land at 1
        // and sit behind A rather than on top of it.
        remove(&conn, b).unwrap();
        add(&conn, c, None).unwrap();

        assert_eq!(order(&conn), vec![a, c]);
    }

    #[test]
    fn reorder_rewrites_the_whole_list() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        let c = entry(&conn, 3, "C");
        for id in [a, b, c] {
            add(&conn, id, None).unwrap();
        }

        reorder(&conn, &[c, a, b]).unwrap();

        assert_eq!(order(&conn), vec![c, a, b]);
    }

    #[test]
    fn deleting_the_library_entry_takes_the_queue_row_with_it() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        add(&conn, a, None).unwrap();
        add(&conn, b, None).unwrap();

        conn.execute("DELETE FROM entry WHERE id = ?1", [a])
            .unwrap();

        // The cascade is what enforces "only games in my library"; without it
        // the row would linger and fail to read.
        assert_eq!(order(&conn), vec![b]);
    }

    #[test]
    fn an_owned_row_carries_no_prices() {
        let conn = db();
        let a = owned_entry(&conn, 1, "A", true);
        offer(&conn, 1, "Steam", 19.99);
        add(&conn, a, None).unwrap();

        let row = &list(&conn, None).unwrap()[0];

        // Bought already: what it costs is a question answered, so the row
        // skips the whole assembly — and can never be "out of date".
        assert!(row.stores.is_empty());
        assert_eq!(row.fetched_at, None);
        assert!(!row.stale);
    }

    #[test]
    fn the_cheapest_offer_comes_first() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        offer(&conn, 1, "Steam", 19.99);
        offer(&conn, 1, "GOG", 9.99);
        add(&conn, a, None).unwrap();

        let row = &list(&conn, None).unwrap()[0];
        let first = row.stores.first().expect("offers should be listed");

        // The picker shows this order, and an unset `preferred_shop` means
        // "the first one" — so cheapest-first is the default, not a tie-break.
        assert_eq!(first.shop, "GOG");
        assert_eq!(first.offer.as_ref().unwrap().price, 9.99);
        assert!(!row.stale, "a snapshot written just now is fresh");
    }

    #[test]
    fn a_store_with_no_listing_still_reports_its_next_sale() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        offer(&conn, 1, "GOG", 9.99);
        add(&conn, a, None).unwrap();

        let row = &list(&conn, None).unwrap()[0];
        let steam = row
            .stores
            .iter()
            .find(|s| s.shop == "Steam")
            .expect("Steam should appear on its sale alone");

        // Dropping a store that does not sell the game would hide the answer
        // to "should I wait?" — which is the whole point of the calendar.
        assert!(steam.offer.is_none());
        assert!(steam.outlook.is_some());

        // And it sorts behind the store that actually has a price.
        assert_eq!(row.stores[0].shop, "GOG");
    }

    #[test]
    fn a_row_with_no_prices_at_all_is_stale() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        add(&conn, a, None).unwrap();

        let row = &list(&conn, None).unwrap()[0];
        assert_eq!(row.fetched_at, None);
        assert!(row.stale, "never fetched is as stale as it gets");
    }

    #[test]
    fn the_preferred_shop_round_trips_and_can_be_cleared() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        add(&conn, a, None).unwrap();

        set_shop(&conn, a, Some("GOG".into())).unwrap();
        assert_eq!(
            list(&conn, None).unwrap()[0].preferred_shop.as_deref(),
            Some("GOG")
        );

        set_shop(&conn, a, None).unwrap();
        assert_eq!(list(&conn, None).unwrap()[0].preferred_shop, None);
    }

    #[test]
    fn queueing_something_not_in_the_library_is_a_clear_error() {
        let conn = db();
        assert!(matches!(add(&conn, 404, None), Err(AppError::NotFound(_))));
    }
}
