//! The Play Next list: an ordered queue of what to play, drawn from games
//! already in the library.
//!
//! Kept separate from the board on purpose. `entry.priority` orders the four
//! columns; this list has its own `position` so reordering one never disturbs
//! the other. A queued game may be unowned, owned-and-unplayed, or finished
//! and worth revisiting — the queue answers "what next?", not "do I own it?".

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

use crate::commands::library::read_entry;
use crate::db::models::LibraryEntry;
use crate::db::now;
use crate::error::{AppError, Result};
use crate::state::AppState;

/// One row of the list. Phase 5b adds the per-store offers; until then the
/// row draws itself from the entry alone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueRow {
    pub entry: LibraryEntry,
    /// Which store's price this row shows once prices land. `None` means the
    /// frontend picks a sensible default.
    pub preferred_shop: Option<String>,
    pub added_at: i64,
}

/// The queue in display order, newest positions last.
pub fn list(conn: &Connection) -> Result<Vec<QueueRow>> {
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

    let mut out = Vec::new();
    for row in rows {
        let (entry_id, preferred_shop, added_at) = row?;
        match read_entry(conn, entry_id) {
            Ok(entry) => out.push(QueueRow {
                entry,
                preferred_shop,
                added_at,
            }),
            // The foreign key makes this unreachable in practice, but a single
            // bad row must not blank the whole page.
            Err(e) => log::warn!("queued entry {entry_id} could not be read: {e}"),
        }
    }
    Ok(out)
}

fn read_row(conn: &Connection, entry_id: i64) -> Result<QueueRow> {
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

    Ok(QueueRow {
        entry: read_entry(conn, entry_id)?,
        preferred_shop,
        added_at,
    })
}

/// Append to the end of the list.
///
/// The end, not the top: the first slot means "this is what I play next" and
/// should not be taken by whatever was added last. (`add_entry` does the
/// opposite for the board, where a new game genuinely is the freshest thing.)
///
/// Idempotent — queueing something already queued returns the existing row
/// rather than moving it, so a double click cannot reshuffle the list.
pub fn add(conn: &Connection, entry_id: i64) -> Result<QueueRow> {
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

    read_row(conn, entry_id)
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
    state.db.with(list)
}

#[tauri::command]
pub fn enqueue(state: State<'_, AppState>, entry_id: i64) -> Result<QueueRow> {
    state.db.with(|conn| add(conn, entry_id))
}

#[tauri::command]
pub fn dequeue(state: State<'_, AppState>, entry_id: i64) -> Result<()> {
    state.db.with(|conn| remove(conn, entry_id))
}

#[tauri::command]
pub fn reorder_queue(state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    state.db.with(|conn| reorder(conn, &ids))
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
        conn.execute(
            "INSERT INTO game (id, name, metadata_fetched) VALUES (?1, ?2, 0)",
            params![igdb_id, name],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entry (game_id, owned, status, priority, added_at, updated_at)
             VALUES (?1, 0, 'want', 0, 0, 0)",
            [igdb_id],
        )
        .unwrap();
        conn.query_row("SELECT id FROM entry WHERE game_id = ?1", [igdb_id], |r| {
            r.get(0)
        })
        .unwrap()
    }

    fn order(conn: &Connection) -> Vec<i64> {
        list(conn)
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

        add(&conn, a).unwrap();
        add(&conn, b).unwrap();
        add(&conn, c).unwrap();

        assert_eq!(order(&conn), vec![a, b, c]);
    }

    #[test]
    fn queueing_twice_does_not_move_the_row() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        add(&conn, a).unwrap();
        add(&conn, b).unwrap();

        // A double click must not shunt A to the back of the list.
        add(&conn, a).unwrap();

        assert_eq!(order(&conn), vec![a, b]);
    }

    #[test]
    fn a_gap_left_by_a_removal_does_not_collide_with_the_next_add() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        let c = entry(&conn, 3, "C");
        add(&conn, a).unwrap();
        add(&conn, b).unwrap();

        // Removing the last row leaves MAX(position) = 0, so C must land at 1
        // and sit behind A rather than on top of it.
        remove(&conn, b).unwrap();
        add(&conn, c).unwrap();

        assert_eq!(order(&conn), vec![a, c]);
    }

    #[test]
    fn reorder_rewrites_the_whole_list() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        let c = entry(&conn, 3, "C");
        for id in [a, b, c] {
            add(&conn, id).unwrap();
        }

        reorder(&conn, &[c, a, b]).unwrap();

        assert_eq!(order(&conn), vec![c, a, b]);
    }

    #[test]
    fn deleting_the_library_entry_takes_the_queue_row_with_it() {
        let conn = db();
        let a = entry(&conn, 1, "A");
        let b = entry(&conn, 2, "B");
        add(&conn, a).unwrap();
        add(&conn, b).unwrap();

        conn.execute("DELETE FROM entry WHERE id = ?1", [a])
            .unwrap();

        // The cascade is what enforces "only games in my library"; without it
        // the row would linger and fail to read.
        assert_eq!(order(&conn), vec![b]);
    }

    #[test]
    fn queueing_something_not_in_the_library_is_a_clear_error() {
        let conn = db();
        assert!(matches!(add(&conn, 404), Err(AppError::NotFound(_))));
    }
}
