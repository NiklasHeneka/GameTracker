use rusqlite::Connection;

use crate::error::Result;

/// Ordered, append-only. Index + 1 is the schema version recorded in
/// `PRAGMA user_version`. Never edit a migration that has shipped — add a
/// new one instead.
const MIGRATIONS: &[&str] = &[
    // ── 001 — core schema ────────────────────────────────────────────────
    r#"
    -- Generic key/value store: app settings, cached OAuth tokens, counters.
    CREATE TABLE setting (
      key   TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );

    -- Metadata cache, keyed by IGDB id.
    CREATE TABLE game (
      id               INTEGER PRIMARY KEY,
      name             TEXT    NOT NULL,
      slug             TEXT,
      summary          TEXT,
      cover_image_id   TEXT,
      artwork_image_id TEXT,
      first_release    INTEGER,
      igdb_rating      REAL,
      steam_appid      INTEGER,
      itad_uuid        TEXT,
      cheapshark_id    TEXT,
      metadata_fetched INTEGER NOT NULL
    );
    CREATE INDEX idx_game_name        ON game (name);
    CREATE INDEX idx_game_steam_appid ON game (steam_appid) WHERE steam_appid IS NOT NULL;

    CREATE TABLE genre (
      id   INTEGER PRIMARY KEY,
      name TEXT NOT NULL
    );

    CREATE TABLE platform (
      id           INTEGER PRIMARY KEY,
      name         TEXT NOT NULL,
      abbreviation TEXT,
      -- pc | playstation | nintendo | xbox | other; drives price-source routing
      family       TEXT NOT NULL DEFAULT 'other'
    );

    CREATE TABLE game_genre (
      game_id  INTEGER NOT NULL REFERENCES game(id)  ON DELETE CASCADE,
      genre_id INTEGER NOT NULL REFERENCES genre(id) ON DELETE CASCADE,
      PRIMARY KEY (game_id, genre_id)
    );

    CREATE TABLE game_platform (
      game_id     INTEGER NOT NULL REFERENCES game(id)     ON DELETE CASCADE,
      platform_id INTEGER NOT NULL REFERENCES platform(id) ON DELETE CASCADE,
      PRIMARY KEY (game_id, platform_id)
    );

    -- The user's list. One row per tracked game.
    CREATE TABLE entry (
      id             INTEGER PRIMARY KEY AUTOINCREMENT,
      game_id        INTEGER NOT NULL UNIQUE REFERENCES game(id) ON DELETE CASCADE,
      owned          INTEGER NOT NULL DEFAULT 0 CHECK (owned IN (0, 1)),
      status         TEXT    NOT NULL DEFAULT 'want'
                     CHECK (status IN ('want', 'playing', 'finished', 'dropped')),
      priority       INTEGER NOT NULL DEFAULT 0,
      own_platform   TEXT,
      target_price   REAL,
      price_at_add   REAL,
      purchase_price REAL,
      purchase_date  INTEGER,
      purchase_store TEXT,
      hours_played   REAL,
      my_rating      INTEGER CHECK (my_rating IS NULL OR my_rating BETWEEN 1 AND 10),
      notes          TEXT,
      source         TEXT NOT NULL DEFAULT 'manual',
      added_at       INTEGER NOT NULL,
      updated_at     INTEGER NOT NULL,
      started_at     INTEGER,
      finished_at    INTEGER
    );
    CREATE INDEX idx_entry_bucket ON entry (owned, status, priority);
    "#,
    // ── 002 — company credits, shown in the game detail panel ────────────
    r#"
    ALTER TABLE game ADD COLUMN developer TEXT;
    ALTER TABLE game ADD COLUMN publisher TEXT;
    "#,
    // ── 003 — prices ────────────────────────────────────────────────────
    r#"
    -- Current best price per shop. Upserted on every refresh.
    CREATE TABLE price_snapshot (
      game_id         INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
      shop            TEXT    NOT NULL,
      country         TEXT    NOT NULL,
      -- pc | playstation | nintendo | xbox — the deals panel groups by this
      platform_family TEXT    NOT NULL DEFAULT 'pc',
      currency        TEXT    NOT NULL,
      price           REAL    NOT NULL,
      regular         REAL,
      cut             INTEGER NOT NULL DEFAULT 0,
      url             TEXT,
      drm             TEXT,
      -- When the current sale ends. ITAD supplies it for roughly half of deals.
      sale_expiry     INTEGER,
      -- itad | cheapshark | manual
      source          TEXT    NOT NULL,
      fetched_at      INTEGER NOT NULL,
      PRIMARY KEY (game_id, shop, country)
    );
    CREATE INDEX idx_snapshot_cut ON price_snapshot (cut DESC) WHERE cut > 0;

    -- Historical lows: scope is 'all' | 'y1' | 'm3' | 'shop'.
    CREATE TABLE price_low (
      game_id     INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
      scope       TEXT    NOT NULL,
      shop        TEXT    NOT NULL DEFAULT '',
      country     TEXT    NOT NULL,
      currency    TEXT    NOT NULL,
      price       REAL    NOT NULL,
      cut         INTEGER NOT NULL DEFAULT 0,
      occurred_at INTEGER,
      fetched_at  INTEGER NOT NULL,
      PRIMARY KEY (game_id, scope, shop, country)
    );

    -- Price change log, for the sparkline and "last on sale".
    CREATE TABLE price_history (
      game_id  INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
      country  TEXT    NOT NULL,
      ts       INTEGER NOT NULL,
      shop     TEXT    NOT NULL DEFAULT '',
      currency TEXT    NOT NULL,
      price    REAL    NOT NULL,
      regular  REAL,
      cut      INTEGER NOT NULL DEFAULT 0,
      PRIMARY KEY (game_id, country, ts, shop)
    );

    -- One row per notification sent, so a drop is not announced repeatedly.
    CREATE TABLE alert_log (
      game_id     INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
      shop        TEXT    NOT NULL,
      price       REAL    NOT NULL,
      notified_at INTEGER NOT NULL,
      PRIMARY KEY (game_id, shop, price)
    );
    "#,
    // ── 004 — PlayStation Store concept id ──────────────────────────────
    r#"
    -- Numeric PS Store concept id (e.g. Elden Ring = 10000333), taken from
    -- IGDB's external_games. The join key for PlayStation prices, exactly as
    -- steam_appid is for PC.
    ALTER TABLE game ADD COLUMN psn_concept_id TEXT;
    "#,
];

pub fn run(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let target = MIGRATIONS.len() as i64;

    if current > target {
        log::warn!(
            "database schema is v{current} but this build only knows v{target}; \
             it was probably written by a newer version of GameTracker"
        );
        return Ok(());
    }

    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = idx as i64 + 1;
        log::info!("applying migration {version}");
        // execute_batch runs inside an implicit transaction per statement, so
        // wrap the whole migration to keep it all-or-nothing.
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {version};\nCOMMIT;"
        ))
        .inspect_err(|e| log::error!("migration {version} failed: {e}"))?;
    }

    Ok(())
}
