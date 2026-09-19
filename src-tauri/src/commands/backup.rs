use std::path::PathBuf;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::models::LibraryEntry;
use crate::db::now;
use crate::error::{AppError, Result};
use crate::settings::{self, Settings};
use crate::state::AppState;

/// Bumped only when the shape changes incompatibly. An importer refuses a
/// version it does not understand rather than guessing.
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub app: String,
    pub format_version: u32,
    pub exported_at: i64,
    /// Settings without anything secret — credentials live in `.env` and are
    /// deliberately not part of a backup.
    pub settings: Settings,
    pub entries: Vec<LibraryEntry>,
    /// The Play Next list. `default` because backups written before Phase 5
    /// have no such key — which is also why `FORMAT_VERSION` stays at 1: the
    /// addition is readable by old builds (serde ignores unknown fields) and
    /// by new ones, so refusing either direction would be wrong.
    #[serde(default)]
    pub queue: Vec<QueuedGame>,
}

/// One Play Next row, keyed by **IGDB id rather than `entry_id`**.
///
/// Importing re-inserts entries and they get fresh row ids, so a queue
/// serialised by `entry_id` would restore pointing at the wrong games or at
/// nothing at all. IGDB ids are stable across databases.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedGame {
    pub igdb_id: i64,
    pub position: i64,
    pub preferred_shop: Option<String>,
}

/// The queue in display order, resolved to game ids for export.
fn collect_queue(conn: &rusqlite::Connection) -> Result<Vec<QueuedGame>> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.game_id, q.position, q.preferred_shop
         FROM queue q JOIN entry e ON e.id = q.entry_id
         ORDER BY q.position ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(QueuedGame {
            igdb_id: r.get(0)?,
            position: r.get(1)?,
            preferred_shop: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Append backed-up queue rows to whatever is already queued, keeping their
/// relative order. Additive like the rest of the importer: a game that is not
/// in the library, or is already on the list, is skipped rather than moved.
fn restore_queue(conn: &rusqlite::Connection, queued: &[QueuedGame]) -> Result<usize> {
    let mut ordered: Vec<&QueuedGame> = queued.iter().collect();
    ordered.sort_by_key(|q| q.position);

    let mut next: i64 = conn
        .prepare_cached("SELECT COALESCE(MAX(position), -1) + 1 FROM queue")?
        .query_row([], |r| r.get(0))?;

    let mut added = 0;
    for item in ordered {
        let entry_id: Option<i64> = conn
            .prepare_cached("SELECT id FROM entry WHERE game_id = ?1")?
            .query_row([item.igdb_id], |r| r.get(0))
            .optional()?;
        let Some(entry_id) = entry_id else { continue };

        let inserted = conn
            .prepare_cached(
                "INSERT INTO queue (entry_id, position, preferred_shop, added_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (entry_id) DO NOTHING",
            )?
            .execute(params![entry_id, next, item.preferred_shop, now()])?;

        if inserted > 0 {
            next += 1;
            added += 1;
        }
    }
    Ok(added)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub entries_in_file: usize,
    pub added: usize,
    /// Present already; left exactly as they were.
    pub skipped: usize,
    /// Play Next rows restored.
    pub queued: usize,
    pub exported_at: i64,
    pub settings_applied: bool,
}

#[tauri::command]
pub fn export_backup(state: State<'_, AppState>, path: PathBuf) -> Result<usize> {
    let (settings, entries, queue) = state.db.with(|conn| {
        let settings = settings::read(conn)?;
        let entries = crate::commands::library::all_entries(conn)?;
        let queue = collect_queue(conn)?;
        Ok((settings, entries, queue))
    })?;

    let backup = Backup {
        app: "GameTracker".into(),
        format_version: FORMAT_VERSION,
        exported_at: now(),
        settings,
        entries,
        queue,
    };

    let json = serde_json::to_string_pretty(&backup)?;
    std::fs::write(&path, &json)?;
    log::info!(
        "exported {} entries to {}",
        backup.entries.len(),
        path.display()
    );
    Ok(backup.entries.len())
}

/// Restore from a backup. Additive: a game already tracked is left untouched,
/// so importing into a live library can only add, never overwrite.
#[tauri::command]
pub fn import_backup(
    state: State<'_, AppState>,
    path: PathBuf,
    apply_settings: bool,
) -> Result<ImportSummary> {
    let text = std::fs::read_to_string(&path)?;
    let backup: Backup = serde_json::from_str(&text)
        .map_err(|e| AppError::Config(format!("That file is not a GameTracker backup ({e}).")))?;

    if backup.format_version > FORMAT_VERSION {
        return Err(AppError::Config(format!(
            "This backup is version {} but this build only understands version {}. \
             Update GameTracker first.",
            backup.format_version, FORMAT_VERSION
        )));
    }

    let mut summary = ImportSummary {
        entries_in_file: backup.entries.len(),
        added: 0,
        skipped: 0,
        queued: 0,
        exported_at: backup.exported_at,
        settings_applied: false,
    };

    state.db.with(|conn| {
        let tx = conn.unchecked_transaction()?;

        for entry in &backup.entries {
            let game = &entry.game;

            // A backup carries only the summary, so metadata is written from
            // what it holds; a later refresh fills in the rest from IGDB.
            tx.prepare_cached(
                "INSERT INTO game (id, name, cover_image_id, first_release, steam_appid,
                                   metadata_fetched)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)
                 ON CONFLICT (id) DO NOTHING",
            )?
            .execute(params![
                game.igdb_id,
                game.name,
                game.cover_image_id,
                game.first_release,
                game.steam_appid
            ])?;

            let exists: Option<i64> = tx
                .prepare_cached("SELECT id FROM entry WHERE game_id = ?1")?
                .query_row([game.igdb_id], |r| r.get(0))
                .optional()?;

            if exists.is_some() {
                summary.skipped += 1;
                continue;
            }

            tx.prepare_cached(
                "INSERT INTO entry (game_id, owned, status, priority, own_platform, target_price,
                                    price_at_add, purchase_price, purchase_date, purchase_store,
                                    hours_played, my_rating, notes, source, added_at, updated_at,
                                    started_at, finished_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'backup',?14,?15,?16,?17)",
            )?
            .execute(params![
                game.igdb_id,
                entry.owned as i64,
                entry.status,
                entry.priority,
                entry.own_platform,
                entry.target_price,
                entry.price_at_add,
                entry.purchase_price,
                entry.purchase_date,
                entry.purchase_store,
                entry.hours_played,
                entry.my_rating,
                entry.notes,
                entry.added_at,
                entry.updated_at,
                entry.started_at,
                entry.finished_at,
            ])?;
            summary.added += 1;
        }

        // After the entries, so every game the file carries can be resolved.
        summary.queued = restore_queue(&tx, &backup.queue)?;

        if apply_settings {
            settings::write(&tx, &backup.settings)?;
            summary.settings_applied = true;
        }

        tx.commit()?;
        Ok(())
    })?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn a_backup_round_trips_through_json() {
        let conn = db();
        let backup = Backup {
            app: "GameTracker".into(),
            format_version: FORMAT_VERSION,
            exported_at: 1_700_000_000,
            settings: settings::read(&conn).unwrap(),
            entries: Vec::new(),
            queue: Vec::new(),
        };
        let json = serde_json::to_string(&backup).unwrap();
        let back: Backup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.format_version, FORMAT_VERSION);
        assert_eq!(back.exported_at, 1_700_000_000);
        assert_eq!(back.settings.currency, backup.settings.currency);
    }

    #[test]
    fn a_newer_format_is_refused_rather_than_guessed_at() {
        // Hand-built because the struct cannot express a future version.
        let json = r#"{"app":"GameTracker","formatVersion":99,"exportedAt":0,
                       "settings":{},"entries":[]}"#;
        let parsed: Backup = serde_json::from_str(json).unwrap();
        assert!(parsed.format_version > FORMAT_VERSION);
    }

    /// Insert a game plus its library entry, returning the new entry id.
    fn entry(conn: &Connection, igdb_id: i64) -> i64 {
        conn.execute(
            "INSERT INTO game (id, name, metadata_fetched) VALUES (?1, ?2, 0)
             ON CONFLICT (id) DO NOTHING",
            params![igdb_id, format!("Game {igdb_id}")],
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

    #[test]
    fn the_queue_survives_entries_being_re_inserted_with_new_ids() {
        let conn = db();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();

        for id in [10, 20, 30] {
            entry(&conn, id);
        }
        // Queue them in an order that is not the insertion order, so a bug
        // that falls back to insertion order cannot pass by accident.
        restore_queue(
            &conn,
            &[
                QueuedGame {
                    igdb_id: 30,
                    position: 0,
                    preferred_shop: None,
                },
                QueuedGame {
                    igdb_id: 10,
                    position: 1,
                    preferred_shop: Some("Steam".into()),
                },
                QueuedGame {
                    igdb_id: 20,
                    position: 2,
                    preferred_shop: None,
                },
            ],
        )
        .unwrap();

        let exported = collect_queue(&conn).unwrap();
        let json = serde_json::to_string(&exported).unwrap();

        // Restoring into a fresh library: the entries come back with different
        // row ids, which is exactly what a queue keyed on entry_id would break
        // on. AUTOINCREMENT guarantees the ids really are new.
        conn.execute("DELETE FROM entry", []).unwrap();
        assert!(
            collect_queue(&conn).unwrap().is_empty(),
            "cascade should have cleared the queue"
        );
        let fresh: Vec<i64> = [20, 30, 10].iter().map(|id| entry(&conn, *id)).collect();
        assert!(
            fresh.iter().all(|id| *id > 3),
            "entry ids should not be reused"
        );

        let parsed: Vec<QueuedGame> = serde_json::from_str(&json).unwrap();
        assert_eq!(restore_queue(&conn, &parsed).unwrap(), 3);

        let restored = collect_queue(&conn).unwrap();
        let order: Vec<i64> = restored.iter().map(|q| q.igdb_id).collect();
        assert_eq!(order, vec![30, 10, 20]);
        assert_eq!(restored[1].preferred_shop.as_deref(), Some("Steam"));
    }

    #[test]
    fn restoring_skips_games_the_library_does_not_have() {
        let conn = db();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        entry(&conn, 10);

        let added = restore_queue(
            &conn,
            &[
                QueuedGame {
                    igdb_id: 10,
                    position: 0,
                    preferred_shop: None,
                },
                QueuedGame {
                    igdb_id: 99,
                    position: 1,
                    preferred_shop: None,
                },
            ],
        )
        .unwrap();

        // 99 was never imported — skipped, not an error, and it must not leave
        // a hole that shifts everything after it.
        assert_eq!(added, 1);
        assert_eq!(collect_queue(&conn).unwrap().len(), 1);
    }

    #[test]
    fn settings_carry_no_credentials() {
        let conn = db();
        let json = serde_json::to_string(&settings::read(&conn).unwrap()).unwrap();
        for secret in ["apiKey", "clientSecret", "IGDB", "ITAD_", "STEAM_API"] {
            assert!(!json.contains(secret), "a backup must not carry {secret}");
        }
    }
}
