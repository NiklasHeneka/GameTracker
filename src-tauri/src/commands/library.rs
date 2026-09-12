use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;

use crate::db::models::{EntryPatch, LibraryEntry, STATUSES};
use crate::db::now;
use crate::error::{AppError, Result};
use crate::services::metadata;
use crate::state::AppState;

const COLUMNS: &str = "id, game_id, owned, status, priority, own_platform, target_price,
    price_at_add, purchase_price, purchase_date, purchase_store, hours_played, my_rating,
    notes, added_at, updated_at, started_at, finished_at";

/// Row → domain object. The game summary is a second query per entry; at
/// library scale (hundreds, not millions) that is far cheaper than the
/// multi-join it would replace, and it keeps one source of truth for the shape.
fn hydrate(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, LibraryEntry)> {
    let game_id: i64 = row.get(1)?;
    Ok((
        game_id,
        LibraryEntry {
            id: row.get(0)?,
            // Filled in by the caller, which can surface a real error.
            game: crate::db::models::GameSummary {
                igdb_id: game_id,
                name: String::new(),
                cover_image_id: None,
                first_release: None,
                steam_appid: None,
                genres: Vec::new(),
                platforms: Vec::new(),
            },
            owned: row.get::<_, i64>(2)? != 0,
            status: row.get(3)?,
            priority: row.get(4)?,
            own_platform: row.get(5)?,
            target_price: row.get(6)?,
            price_at_add: row.get(7)?,
            purchase_price: row.get(8)?,
            purchase_date: row.get(9)?,
            purchase_store: row.get(10)?,
            hours_played: row.get(11)?,
            my_rating: row.get(12)?,
            notes: row.get(13)?,
            added_at: row.get(14)?,
            updated_at: row.get(15)?,
            started_at: row.get(16)?,
            finished_at: row.get(17)?,
        },
    ))
}

fn read_entry(conn: &Connection, id: i64) -> Result<LibraryEntry> {
    let found = conn
        .prepare_cached(&format!("SELECT {COLUMNS} FROM entry WHERE id = ?1"))?
        .query_row([id], |r| hydrate(r))
        .optional()?;

    let Some((game_id, mut entry)) = found else {
        return Err(AppError::NotFound(format!("library entry {id}")));
    };

    entry.game = metadata::read_summary(conn, game_id)?
        .ok_or_else(|| AppError::NotFound(format!("game {game_id}")))?;
    Ok(entry)
}

#[tauri::command]
pub fn list_entries(state: State<'_, AppState>) -> Result<Vec<LibraryEntry>> {
    state.db.with(|conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM entry ORDER BY priority ASC, added_at DESC"
        ))?;
        let rows = stmt.query_map([], |r| hydrate(r))?;

        let mut out = Vec::new();
        for row in rows {
            let (game_id, mut entry) = row?;
            match metadata::read_summary(conn, game_id)? {
                Some(summary) => {
                    entry.game = summary;
                    out.push(entry);
                }
                // A library entry whose game row is missing would be invisible
                // and unfixable in the UI; log it rather than failing the list.
                None => log::warn!("entry {} references missing game {game_id}", entry.id),
            }
        }
        Ok(out)
    })
}

#[tauri::command]
pub async fn add_entry(
    state: State<'_, AppState>,
    igdb_id: i64,
    owned: bool,
    own_platform: Option<String>,
) -> Result<LibraryEntry> {
    let known = state.db.with(|conn| {
        Ok(conn
            .prepare_cached("SELECT 1 FROM game WHERE id = ?1")?
            .exists([igdb_id])?)
    })?;

    if !known {
        let game = state
            .igdb()?
            .game(igdb_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("IGDB has no game {igdb_id}")))?;
        state.db.with(|conn| metadata::upsert_game(conn, &game))?;
    }

    state.db.with(|conn| {
        let ts = now();
        // New entries sort to the top of their column.
        let top: i64 = conn
            .prepare_cached("SELECT COALESCE(MIN(priority), 0) - 1 FROM entry")?
            .query_row([], |r| r.get(0))?;

        conn.prepare_cached(
            "INSERT INTO entry (game_id, owned, status, priority, own_platform,
                                purchase_date, added_at, updated_at)
             VALUES (?1, ?2, 'want', ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (game_id) DO NOTHING",
        )?
        .execute(params![
            igdb_id,
            owned as i64,
            top,
            own_platform,
            owned.then_some(ts),
            ts
        ])?;

        let id: i64 = conn
            .prepare_cached("SELECT id FROM entry WHERE game_id = ?1")?
            .query_row([igdb_id], |r| r.get(0))?;
        read_entry(conn, id)
    })
}

#[tauri::command]
pub fn update_entry(state: State<'_, AppState>, id: i64, patch: EntryPatch) -> Result<LibraryEntry> {
    state.db.with(|conn| {
        let mut entry = read_entry(conn, id)?;
        let ts = now();

        if let Some(v) = patch.status {
            if !STATUSES.contains(&v.as_str()) {
                return Err(AppError::Config(format!("unknown status {v:?}")));
            }
            // Stamp the milestones the first time each is reached, so the
            // stats view has real dates without the user entering them.
            if v == "playing" && entry.started_at.is_none() {
                entry.started_at = Some(ts);
            }
            if v == "finished" && entry.finished_at.is_none() {
                entry.finished_at = Some(ts);
            }
            entry.status = v;
        }

        if let Some(v) = patch.owned {
            // Buying it is the moment worth recording; the stats view compares
            // this against price_at_add to show what waiting saved.
            if v && !entry.owned && entry.purchase_date.is_none() {
                entry.purchase_date = Some(ts);
            }
            entry.owned = v;
        }

        if let Some(v) = patch.own_platform {
            entry.own_platform = v;
        }
        if let Some(v) = patch.target_price {
            entry.target_price = v;
        }
        if let Some(v) = patch.purchase_price {
            entry.purchase_price = v;
        }
        if let Some(v) = patch.purchase_date {
            entry.purchase_date = v;
        }
        if let Some(v) = patch.purchase_store {
            entry.purchase_store = v;
        }
        if let Some(v) = patch.hours_played {
            entry.hours_played = v;
        }
        if let Some(v) = patch.my_rating {
            entry.my_rating = v;
        }
        if let Some(v) = patch.notes {
            entry.notes = v.filter(|s| !s.trim().is_empty());
        }

        conn.prepare_cached(
            "UPDATE entry SET owned = ?2, status = ?3, own_platform = ?4, target_price = ?5,
                              purchase_price = ?6, purchase_date = ?7, purchase_store = ?8,
                              hours_played = ?9, my_rating = ?10, notes = ?11,
                              started_at = ?12, finished_at = ?13, updated_at = ?14
             WHERE id = ?1",
        )?
        .execute(params![
            id,
            entry.owned as i64,
            entry.status,
            entry.own_platform,
            entry.target_price,
            entry.purchase_price,
            entry.purchase_date,
            entry.purchase_store,
            entry.hours_played,
            entry.my_rating,
            entry.notes,
            entry.started_at,
            entry.finished_at,
            ts,
        ])?;

        read_entry(conn, id)
    })
}

/// Persist a manual drag-and-drop ordering.
#[tauri::command]
pub fn reorder_entries(state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    state.db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE entry SET priority = ?2 WHERE id = ?1")?;
            for (position, id) in ids.iter().enumerate() {
                stmt.execute(params![id, position as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

#[tauri::command]
pub fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.db.with(|conn| {
        let affected = conn
            .prepare_cached("DELETE FROM entry WHERE id = ?1")?
            .execute([id])?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("library entry {id}")));
        }
        Ok(())
    })
}
