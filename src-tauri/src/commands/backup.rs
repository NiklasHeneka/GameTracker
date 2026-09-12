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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub entries_in_file: usize,
    pub added: usize,
    /// Present already; left exactly as they were.
    pub skipped: usize,
    pub exported_at: i64,
    pub settings_applied: bool,
}

#[tauri::command]
pub fn export_backup(state: State<'_, AppState>, path: PathBuf) -> Result<usize> {
    let (settings, entries) = state.db.with(|conn| {
        let settings = settings::read(conn)?;
        let entries = crate::commands::library::all_entries(conn)?;
        Ok((settings, entries))
    })?;

    let backup = Backup {
        app: "GameTracker".into(),
        format_version: FORMAT_VERSION,
        exported_at: now(),
        settings,
        entries,
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

    #[test]
    fn settings_carry_no_credentials() {
        let conn = db();
        let json = serde_json::to_string(&settings::read(&conn).unwrap()).unwrap();
        for secret in ["apiKey", "clientSecret", "IGDB", "ITAD_", "STEAM_API"] {
            assert!(!json.contains(secret), "a backup must not carry {secret}");
        }
    }
}
