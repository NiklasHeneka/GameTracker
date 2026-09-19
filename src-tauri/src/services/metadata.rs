use rusqlite::{params, Connection, OptionalExtension};

use crate::clients::igdb::{IgdbGame, TimeToBeatRow};
use crate::db::models::{GameDetail, GameSummary, PlatformRef, TimeToBeat};
use crate::db::now;
use crate::error::Result;

/// Which price source a platform belongs to.
///
/// IGDB's own `platform_family` covers the consoles (1 PlayStation, 2 Xbox,
/// 5 Nintendo) but leaves desktop platforms unset — and Linux reports family 4
/// — so the explicit desktop ids are checked first.
pub fn platform_family(platform_id: i64, igdb_family: Option<i64>) -> &'static str {
    match platform_id {
        6 | 13 | 14 | 3 | 163 => "pc", // Windows, DOS, Mac, Linux, SteamVR
        _ => match igdb_family {
            Some(1) => "playstation",
            Some(2) => "xbox",
            Some(5) => "nintendo",
            _ => "other",
        },
    }
}

/// Write a game and its genre/platform links. Idempotent: re-fetching a game
/// refreshes its metadata without disturbing the library entry pointing at it.
pub fn upsert_game(conn: &Connection, g: &IgdbGame) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO game (id, name, slug, summary, cover_image_id, artwork_image_id,
                           first_release, igdb_rating, steam_appid, developer, publisher,
                           metadata_fetched, psn_concept_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (id) DO UPDATE SET
           name = excluded.name, slug = excluded.slug, summary = excluded.summary,
           cover_image_id = excluded.cover_image_id,
           artwork_image_id = excluded.artwork_image_id,
           first_release = excluded.first_release, igdb_rating = excluded.igdb_rating,
           -- Never overwrite a known Steam appid with a null from a partial fetch.
           steam_appid = COALESCE(excluded.steam_appid, game.steam_appid),
           psn_concept_id = COALESCE(excluded.psn_concept_id, game.psn_concept_id),
           developer = excluded.developer, publisher = excluded.publisher,
           metadata_fetched = excluded.metadata_fetched",
    )?
    .execute(params![
        g.id,
        g.name,
        g.slug,
        g.summary,
        g.cover_image_id(),
        g.artwork_image_id(),
        g.first_release_date,
        g.total_rating,
        g.steam_appid(),
        g.company(true),
        g.company(false),
        now(),
        g.psn_concept_id(),
    ])?;

    // Genres and platforms are replaced wholesale — IGDB is the authority and
    // an entry removed upstream should disappear here too.
    conn.execute("DELETE FROM game_genre WHERE game_id = ?1", [g.id])?;
    for (idx, genre) in g.genres.iter().enumerate() {
        // IGDB genre ids are not requested (only names), so derive a stable
        // local id from the name to keep the join table normalised.
        let genre_id = stable_id(&genre.name);
        conn.prepare_cached("INSERT OR IGNORE INTO genre (id, name) VALUES (?1, ?2)")?
            .execute(params![genre_id, genre.name])?;
        conn.prepare_cached(
            "INSERT OR IGNORE INTO game_genre (game_id, genre_id) VALUES (?1, ?2)",
        )?
        .execute(params![g.id, genre_id])?;
        let _ = idx;
    }

    conn.execute("DELETE FROM game_platform WHERE game_id = ?1", [g.id])?;
    for p in &g.platforms {
        conn.prepare_cached(
            "INSERT INTO platform (id, name, abbreviation, family) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
               name = excluded.name, abbreviation = excluded.abbreviation,
               family = excluded.family",
        )?
        .execute(params![
            p.id,
            p.name,
            p.abbreviation,
            platform_family(p.id, p.platform_family)
        ])?;
        conn.prepare_cached(
            "INSERT OR IGNORE INTO game_platform (game_id, platform_id) VALUES (?1, ?2)",
        )?
        .execute(params![g.id, p.id])?;
    }

    Ok(())
}

/// FNV-1a over the genre name, masked to stay inside SQLite's signed integer
/// range. Genre names are a small, stable vocabulary, so collisions are not a
/// practical concern.
fn stable_id(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    (hash & 0x7fff_ffff) as i64
}

fn genres_of(conn: &Connection, game_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT g.name FROM genre g
         JOIN game_genre gg ON gg.genre_id = g.id
         WHERE gg.game_id = ?1 ORDER BY g.name",
    )?;
    let rows = stmt.query_map([game_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn platforms_of(conn: &Connection, game_id: i64) -> Result<Vec<PlatformRef>> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.id, p.name, p.abbreviation, p.family FROM platform p
         JOIN game_platform gp ON gp.platform_id = p.id
         WHERE gp.game_id = ?1
         -- Group by the families the user buys on, then lead each family with
         -- its flagship: alphabetical alone puts Linux ahead of Windows, so a
         -- card chip would name Linux for a game everyone plays on PC.
         ORDER BY CASE p.family
                    WHEN 'pc' THEN 0 WHEN 'playstation' THEN 1
                    WHEN 'nintendo' THEN 2 WHEN 'xbox' THEN 3 ELSE 4 END,
                  CASE p.id
                    WHEN 6 THEN 0     -- PC (Microsoft Windows)
                    WHEN 167 THEN 0   -- PlayStation 5
                    WHEN 130 THEN 0   -- Nintendo Switch
                    WHEN 169 THEN 0   -- Xbox Series X|S
                    ELSE 1 END,
                  p.name",
    )?;
    let rows = stmt.query_map([game_id], |r| {
        Ok(PlatformRef {
            id: r.get(0)?,
            name: r.get(1)?,
            abbreviation: r.get(2)?,
            family: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Store one game's playtimes. A game IGDB has no row for is stamped with a
/// count of 0, so it is not asked about again on every launch.
pub fn save_time_to_beat(conn: &Connection, rows: &[TimeToBeatRow], asked: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "UPDATE game SET ttb_hastily = ?2, ttb_normally = ?3, ttb_completely = ?4,
                             ttb_count = ?5
             WHERE id = ?1",
        )?;
        for row in rows {
            stmt.execute(params![
                row.game_id,
                row.hastily,
                row.normally,
                row.completely,
                row.count,
            ])?;
        }

        let answered: std::collections::HashSet<i64> = rows.iter().map(|r| r.game_id).collect();
        let mut blank =
            tx.prepare_cached("UPDATE game SET ttb_count = 0 WHERE id = ?1 AND ttb_count IS NULL")?;
        for id in asked.iter().filter(|id| !answered.contains(id)) {
            blank.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Ask IGDB about every tracked game whose playtime has never been looked up.
///
/// One request covers 200 games, and a game is stamped either way, so this is
/// a no-op on every pass after the first. Returns how many had an answer.
pub async fn backfill_time_to_beat(state: &crate::state::AppState) -> Result<usize> {
    let ids = state.db.with(games_missing_time_to_beat)?;
    if ids.is_empty() {
        return Ok(0);
    }

    let rows = state.igdb()?.time_to_beat(&ids).await?;
    let found = rows.len();
    state.db.with(|conn| save_time_to_beat(conn, &rows, &ids))?;
    Ok(found)
}

/// Games that have never been asked about. Tracked games only — there is no
/// point spending requests on metadata nothing displays.
pub fn games_missing_time_to_beat(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT g.id FROM game g
         JOIN entry e ON e.game_id = g.id
         WHERE g.ttb_count IS NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn read_summary(conn: &Connection, game_id: i64) -> Result<Option<GameSummary>> {
    let base = conn
        .prepare_cached(
            "SELECT id, name, cover_image_id, first_release, steam_appid, igdb_rating,
                    ttb_hastily, ttb_normally, ttb_completely, ttb_count
             FROM game WHERE id = ?1",
        )?
        .query_row([game_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                TimeToBeat::new(r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?),
            ))
        })
        .optional()?;

    let Some((id, name, cover, release, appid, rating, ttb)) = base else {
        return Ok(None);
    };

    Ok(Some(GameSummary {
        igdb_id: id,
        name,
        cover_image_id: cover,
        first_release: release,
        steam_appid: appid,
        igdb_rating: rating,
        time_to_beat: ttb,
        genres: genres_of(conn, id)?,
        platforms: platforms_of(conn, id)?,
    }))
}

pub fn read_detail(conn: &Connection, game_id: i64) -> Result<Option<GameDetail>> {
    let Some(summary) = read_summary(conn, game_id)? else {
        return Ok(None);
    };

    let (summary_text, artwork, developer, publisher) = conn
        .prepare_cached(
            "SELECT summary, artwork_image_id, developer, publisher FROM game WHERE id = ?1",
        )?
        .query_row([game_id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;

    Ok(Some(GameDetail {
        summary,
        summary_text,
        artwork_image_id: artwork,
        developer,
        publisher,
    }))
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

    /// Shaped exactly like a real IGDB response, including the fields that
    /// were renamed out from under us (`game_type`, `external_game_source`).
    fn hollow_knight() -> IgdbGame {
        serde_json::from_str(
            r#"{
              "id": 14593,
              "name": "Hollow Knight",
              "slug": "hollow-knight",
              "summary": "A 2D action-adventure.",
              "first_release_date": 1487894400,
              "total_rating": 91.5,
              "total_rating_count": 2237,
              "cover": { "image_id": "co1rgi" },
              "artworks": [{ "image_id": "ar584b" }, { "image_id": "ar584c" }],
              "genres": [{ "name": "Platform" }, { "name": "Adventure" }],
              "platforms": [
                { "id": 6,   "name": "PC (Microsoft Windows)", "abbreviation": "PC" },
                { "id": 130, "name": "Nintendo Switch", "abbreviation": "Switch", "platform_family": 5 },
                { "id": 48,  "name": "PlayStation 4", "abbreviation": "PS4", "platform_family": 1 },
                { "id": 3,   "name": "Linux", "abbreviation": "Linux", "platform_family": 4 }
              ],
              "external_games": [
                { "uid": "48412",  "external_game_source": 3 },
                { "uid": "367520", "external_game_source": 1 },
                { "uid": "1308320804", "external_game_source": 36 }
              ],
              "involved_companies": [
                { "company": { "name": "Team Cherry" }, "developer": true, "publisher": false },
                { "company": { "name": "Skybound Games" }, "developer": false, "publisher": true }
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn steam_appid_comes_from_the_steam_source_not_the_first_entry() {
        // GOG (3) is listed first; picking [0] would store the wrong id and
        // silently break every price lookup downstream.
        assert_eq!(hollow_knight().steam_appid(), Some(367520));
    }

    #[test]
    fn desktop_platforms_beat_igdbs_own_family_field() {
        // Linux reports platform_family = 4, which is not "pc" to IGDB.
        assert_eq!(platform_family(3, Some(4)), "pc");
        assert_eq!(platform_family(6, None), "pc");
        assert_eq!(platform_family(48, Some(1)), "playstation");
        assert_eq!(platform_family(130, Some(5)), "nintendo");
        assert_eq!(platform_family(169, Some(2)), "xbox");
        assert_eq!(platform_family(29, Some(3)), "other");
    }

    fn ttb(game_id: i64, normally: Option<i64>, count: i64) -> TimeToBeatRow {
        TimeToBeatRow {
            game_id,
            hastily: None,
            normally,
            completely: None,
            count,
        }
    }

    #[test]
    fn a_game_igdb_knows_nothing_about_is_not_asked_about_twice() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();
        conn.execute(
            "INSERT INTO entry (game_id, owned, status, priority, added_at, updated_at)
             VALUES (14593, 0, 'want', 0, 0, 0)",
            [],
        )
        .unwrap();

        assert_eq!(games_missing_time_to_beat(&conn).unwrap(), vec![14593]);

        // IGDB answered with nothing for it.
        save_time_to_beat(&conn, &[], &[14593]).unwrap();

        // Stamped as asked, so the backfill stops retrying it every launch.
        assert!(games_missing_time_to_beat(&conn).unwrap().is_empty());
        assert!(read_summary(&conn, 14593)
            .unwrap()
            .unwrap()
            .time_to_beat
            .is_none());
    }

    #[test]
    fn a_playtime_reaches_the_game_summary() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();
        save_time_to_beat(&conn, &[ttb(14593, Some(129_814), 31)], &[14593]).unwrap();

        let found = read_summary(&conn, 14593)
            .unwrap()
            .unwrap()
            .time_to_beat
            .unwrap();
        assert_eq!(found.normally, Some(129_814));
        assert_eq!(found.count, 31);
        assert!(found.trusted);
    }

    #[test]
    fn untracked_games_are_never_asked_about() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();

        // No library entry: nothing displays it, so it is not worth a request.
        assert!(games_missing_time_to_beat(&conn).unwrap().is_empty());
    }

    #[test]
    fn upsert_round_trips_through_the_database() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();

        let detail = read_detail(&conn, 14593).unwrap().expect("game was saved");
        assert_eq!(detail.summary.name, "Hollow Knight");
        assert_eq!(detail.summary.steam_appid, Some(367520));
        assert_eq!(detail.summary.cover_image_id.as_deref(), Some("co1rgi"));
        assert_eq!(detail.artwork_image_id.as_deref(), Some("ar584b"));
        assert_eq!(detail.developer.as_deref(), Some("Team Cherry"));
        assert_eq!(detail.publisher.as_deref(), Some("Skybound Games"));
        assert_eq!(detail.summary.genres, vec!["Adventure", "Platform"]);

        // PC first, then PlayStation, then Nintendo — the order the deals
        // panel groups by.
        let families: Vec<&str> = detail
            .summary
            .platforms
            .iter()
            .map(|p| p.family.as_str())
            .collect();
        assert_eq!(families, vec!["pc", "pc", "playstation", "nintendo"]);

        // Windows must lead the PC group; alphabetically Linux would win, and
        // that string is what a library card shows as the platform chip.
        assert_eq!(
            detail.summary.platforms[0].abbreviation.as_deref(),
            Some("PC")
        );
    }

    #[test]
    fn re_upserting_replaces_links_instead_of_duplicating_them() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();
        upsert_game(&conn, &hollow_knight()).unwrap();

        let detail = read_detail(&conn, 14593).unwrap().unwrap();
        assert_eq!(detail.summary.genres.len(), 2);
        assert_eq!(detail.summary.platforms.len(), 4);
    }

    #[test]
    fn a_partial_refetch_does_not_erase_a_known_steam_appid() {
        let conn = db();
        upsert_game(&conn, &hollow_knight()).unwrap();

        // A search response carries no external_games at all.
        let sparse: IgdbGame = serde_json::from_str(
            r#"{ "id": 14593, "name": "Hollow Knight", "cover": { "image_id": "co1rgi" } }"#,
        )
        .unwrap();
        upsert_game(&conn, &sparse).unwrap();

        let detail = read_detail(&conn, 14593).unwrap().unwrap();
        assert_eq!(detail.summary.steam_appid, Some(367520));
    }
}
