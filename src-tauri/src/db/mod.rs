pub mod migrations;
pub mod models;
pub mod price_models;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::Result;

/// The single SQLite connection, guarded by a mutex.
///
/// Commands are short and local — a pool would add moving parts for no
/// measurable gain at this scale. WAL keeps the background price refresher
/// (Phase 2) from blocking reads.
pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        migrations::run(&conn)?;
        log::info!("database ready at {}", path.display());

        Ok(Self(Mutex::new(conn)))
    }

    /// Run a closure against the connection. Panics from a previous holder
    /// poison the mutex; recover rather than propagating a poisoned lock,
    /// since the connection itself is still usable.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }
}

/// Read a value from the `setting` key/value table.
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare_cached("SELECT value FROM setting WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    Ok(match rows.next()? {
        Some(row) => Some(row.get(0)?),
        None => None,
    })
}

/// Upsert a value into the `setting` key/value table.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO setting (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )?
    .execute((key, value))?;
    Ok(())
}

/// Unix seconds — the timestamp unit used throughout the schema.
#[allow(dead_code)] // used by the library commands from Phase 1 onwards
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
