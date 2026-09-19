use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;

use crate::clients::itad;
use crate::db::now;
use crate::db::price_models::*;
use crate::error::{AppError, Result};
use crate::services::pricing;
use crate::settings;
use crate::state::AppState;

/// A wishlist game that has fallen to or below its target price.
#[derive(Debug, Clone)]
pub struct PriceAlert {
    #[allow(dead_code)] // carried for future deep-linking from the notification
    pub game_id: i64,
    pub name: String,
    pub shop: String,
    pub price: f64,
    pub currency: String,
    pub target: f64,
}

const CHEAPSHARK_NOTE: &str = "Prices in USD from CheapShark. Add an IsThereAnyDeal \
    key to your .env for prices in your own region, sale end dates and price history.";

struct GameKeys {
    steam_appid: Option<i64>,
    itad_uuid: Option<String>,
    cheapshark_id: Option<String>,
    psn_concept_id: Option<String>,
    name: String,
}

fn read_keys(state: &AppState, game_id: i64) -> Result<GameKeys> {
    state.db.with(|conn| {
        conn.prepare_cached(
            "SELECT steam_appid, itad_uuid, cheapshark_id, psn_concept_id, name
             FROM game WHERE id = ?1",
        )?
        .query_row([game_id], |r| {
            Ok(GameKeys {
                steam_appid: r.get(0)?,
                itad_uuid: r.get(1)?,
                cheapshark_id: r.get(2)?,
                psn_concept_id: r.get(3)?,
                name: r.get(4)?,
            })
        })
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("game {game_id}")))
    })
}

/// Fetch and store prices for one game. Returns a user-facing caveat, if any.
async fn refresh_one(state: &AppState, game_id: i64, country: &str) -> Result<Option<String>> {
    let pc = refresh_pc(state, game_id, country).await;
    let ps = refresh_playstation(state, game_id, country).await;

    // A PlayStation failure must not lose the PC prices, and vice versa.
    if let Err(e) = &ps {
        log::warn!("PlayStation price lookup failed for game {game_id}: {e}");
    }
    let pc_note = pc?;
    // Baseline for the "saved by waiting" stat; a no-op after the first time.
    state
        .db
        .with(|conn| pricing::stamp_price_at_add(conn, game_id, country))?;
    Ok(match (pc_note, ps) {
        (Some(note), _) => Some(note),
        (None, Err(e)) => Some(e.to_string()),
        (None, Ok(note)) => note,
    })
}

/// PlayStation prices, kept separate from the PC sources that replace each
/// other wholesale.
async fn refresh_playstation(
    state: &AppState,
    game_id: i64,
    country: &str,
) -> Result<Option<String>> {
    let enabled = state
        .db
        .with(|conn| Ok(settings::read(conn)?.track_playstation))?;
    if !enabled {
        return Ok(None);
    }

    let keys = read_keys(state, game_id)?;
    let Some(concept) = keys.psn_concept_id else {
        // Most PC-only games have no PlayStation listing; that is not an error.
        return Ok(None);
    };

    let locale = crate::clients::ps_store::locale_for(country);
    let client = state.ps_store()?;

    match client.price(&concept, &locale).await? {
        Some(found) => {
            state.db.with(|conn| {
                pricing::upsert_snapshot(
                    conn,
                    game_id,
                    country,
                    &PriceRow {
                        shop: "PlayStation Store".into(),
                        platform_family: "playstation".into(),
                        currency: found.currency.clone(),
                        price: found.price,
                        regular: found.regular,
                        cut: found.cut,
                        url: Some(found.url.clone()),
                        sale_expiry: found.expiry,
                        source: "psstore".into(),
                        is_all_time_low: false,
                    },
                )
            })?;
            Ok(None)
        }
        None => Ok(None),
    }
}

async fn refresh_pc(state: &AppState, game_id: i64, country: &str) -> Result<Option<String>> {
    let keys = read_keys(state, game_id)?;

    if let Some(client) = state.itad() {
        // Resolve and cache ITAD's id. Steam appid is exact; title is the
        // fallback for games with no Steam release.
        let uuid = match keys.itad_uuid {
            Some(existing) => Some(existing),
            None => {
                let found = match keys.steam_appid {
                    Some(appid) => client.lookup_by_appid(appid).await?,
                    None => None,
                };
                let found = match found {
                    Some(id) => Some(id),
                    None => client.lookup_by_title(&keys.name).await?,
                };
                if let Some(id) = &found {
                    state.db.with(|conn| {
                        conn.execute(
                            "UPDATE game SET itad_uuid = ?2 WHERE id = ?1",
                            params![game_id, id],
                        )?;
                        Ok(())
                    })?;
                }
                found
            }
        };

        let Some(uuid) = uuid else {
            return Ok(Some(format!(
                "IsThereAnyDeal has no entry for \u{201c}{}\u{201d}.",
                keys.name
            )));
        };

        let ids = vec![uuid.clone()];

        let priced = client.prices(&ids, country).await?;
        let rows = priced
            .first()
            .map(|g| pricing::rows_from_itad(&g.deals))
            .unwrap_or_default();
        let no_offers = rows.is_empty();
        state
            .db
            .with(|conn| pricing::replace_snapshots(conn, game_id, country, &rows))?;

        if let Some(low) = client.history_lows(&ids, country).await?.into_iter().next() {
            if let Some(l) = low.low {
                state.db.with(|conn| {
                    pricing::save_low(
                        conn,
                        game_id,
                        country,
                        &PriceLow {
                            scope: "all".into(),
                            shop: l.shop.map(|s| s.name),
                            currency: l.price.currency.clone(),
                            price: l.price.amount,
                            cut: l.cut,
                            occurred_at: l.timestamp.as_deref().and_then(itad::parse_timestamp),
                        },
                    )
                })?;
            }
        }

        // Five years is enough to see a genuine seasonal pattern.
        let history = client.history(&uuid, country, 5).await?;
        let points: Vec<pricing::HistoryRow> = history
            .iter()
            .filter_map(|h| {
                let ts = itad::parse_timestamp(&h.timestamp)?;
                let deal = h.deal.as_ref()?;
                Some((
                    ts,
                    h.shop.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
                    deal.price.currency.clone(),
                    deal.price.amount,
                    deal.regular.as_ref().map(|m| m.amount),
                    deal.cut,
                ))
            })
            .collect();
        state
            .db
            .with(|conn| pricing::save_history(conn, game_id, country, &points))?;

        // A resolved game with no offers usually means this entry has been
        // superseded: Skyrim, GTA V and the like stay listed under their
        // original id while the shops now sell a Special/Enhanced/Complete
        // edition as a separate product.
        return Ok(no_offers.then(|| {
            format!(
                "No PC store currently lists \u{201c}{}\u{201d}. A game replaced by a newer \
                 edition is sold under a separate listing, so this one may have no offers. \
                 You can record a price yourself below.",
                keys.name
            )
        }));
    }

    // ── No ITAD key: CheapShark, which is USD-only ────────────────────────
    let cs = state.cheapshark.clone();
    let cs_id = match keys.cheapshark_id {
        Some(existing) => Some(existing),
        None => {
            let found = match keys.steam_appid {
                Some(appid) => cs.id_for_steam_appid(appid).await?,
                None => None,
            };
            if let Some(id) = &found {
                state.db.with(|conn| {
                    conn.execute(
                        "UPDATE game SET cheapshark_id = ?2 WHERE id = ?1",
                        params![game_id, id],
                    )?;
                    Ok(())
                })?;
            }
            found
        }
    };

    let Some(cs_id) = cs_id else {
        return Ok(Some(format!(
            "No PC store listing found for \u{201c}{}\u{201d}.",
            keys.name
        )));
    };

    let names = cs.store_names().await?;
    let detail = cs.game(&cs_id).await?;
    let rows = pricing::rows_from_cheapshark(&detail, &names);
    state
        .db
        .with(|conn| pricing::replace_snapshots(conn, game_id, country, &rows))?;

    if let Some(ever) = &detail.cheapest_price_ever {
        if let Ok(price) = ever.price.parse::<f64>() {
            state.db.with(|conn| {
                pricing::save_low(
                    conn,
                    game_id,
                    country,
                    &PriceLow {
                        scope: "all".into(),
                        shop: None,
                        currency: crate::clients::cheapshark::CURRENCY.into(),
                        price,
                        cut: 0,
                        occurred_at: Some(ever.date),
                    },
                )
            })?;
        }
    }

    Ok(Some(CHEAPSHARK_NOTE.to_string()))
}

#[tauri::command]
pub async fn get_prices(
    state: State<'_, AppState>,
    igdb_id: i64,
    force: bool,
) -> Result<PriceOverview> {
    let (country, shops) = state.db.with(|conn| {
        let s = settings::read(conn)?;
        Ok((s.country, s.enabled_shops))
    })?;
    let dir = state.env_path.parent().map(|p| p.to_path_buf());

    let cached = state
        .db
        .with(|conn| pricing::overview(conn, igdb_id, &country, &shops, dir.as_deref(), None))?;

    if !force && !cached.stale && !cached.groups.is_empty() {
        // Re-derive the caveat so a cached CheapShark result stays labelled.
        let note = state.itad().is_none().then(|| CHEAPSHARK_NOTE.to_string());
        return state
            .db
            .with(|conn| pricing::overview(conn, igdb_id, &country, &shops, dir.as_deref(), note));
    }

    let note = refresh_one(&state, igdb_id, &country).await?;
    state
        .db
        .with(|conn| pricing::overview(conn, igdb_id, &country, &shops, dir.as_deref(), note))
}

#[tauri::command]
pub async fn refresh_wishlist_prices(state: State<'_, AppState>) -> Result<RefreshReport> {
    refresh_all(&state).await
}

/// Prices for the unowned games on the Play Next list.
#[tauri::command]
pub async fn refresh_queue_prices(state: State<'_, AppState>) -> Result<RefreshReport> {
    let targets = state.db.with(queue_targets)?;
    refresh_targets(&state, targets).await
}

/// Games worth spending requests on: anything wishlisted, plus anything on
/// the Play Next list that is not owned yet.
///
/// Owned games are excluded on purpose — every source has a request budget,
/// and it belongs to what the user might still buy. A queued game qualifies
/// even when it is no longer `want`, because its row shows a price.
pub fn watch_targets(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT game_id FROM entry
         WHERE owned = 0
           AND (status = 'want' OR id IN (SELECT entry_id FROM queue))
         ORDER BY priority",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Unowned games on the Play Next list — the focused refresh that page offers,
/// rather than making the user wait on the whole wishlist.
pub fn queue_targets(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.game_id FROM queue q
         JOIN entry e ON e.id = q.entry_id
         WHERE e.owned = 0
         ORDER BY q.position",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Refresh every watched game and raise alerts for anything that fell below
/// its target price. Shared by the command and the background refresher.
pub async fn refresh_all(state: &AppState) -> Result<RefreshReport> {
    let targets = state.db.with(watch_targets)?;
    refresh_targets(state, targets).await
}

/// Refresh a specific set of games.
pub async fn refresh_targets(state: &AppState, targets: Vec<i64>) -> Result<RefreshReport> {
    let (country, notify) = state.db.with(|conn| {
        let s = settings::read(conn)?;
        Ok((s.country, s.notifications_enabled))
    })?;

    let mut report = RefreshReport {
        checked: targets.len(),
        updated: 0,
        failed: 0,
        alerts: 0,
        message: None,
    };

    for game_id in targets {
        match refresh_one(state, game_id, &country).await {
            Ok(note) => {
                report.updated += 1;
                if report.message.is_none() {
                    report.message = note;
                }
            }
            Err(e) => {
                report.failed += 1;
                log::warn!("price refresh failed for game {game_id}: {e}");
            }
        }
        if notify {
            for alert in raise_alerts(state, game_id, &country)? {
                report.alerts += 1;
                state
                    .pending_alerts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(alert);
            }
        }
    }

    Ok(report)
}

/// Record newly-met price targets and describe them for notification.
///
/// Deduplicated on (game, shop, price) so the same deal is never announced
/// twice, while a *further* drop still counts as new.
fn raise_alerts(state: &AppState, game_id: i64, country: &str) -> Result<Vec<PriceAlert>> {
    state.db.with(|conn| {
        let target: Option<f64> = conn
            .prepare_cached("SELECT target_price FROM entry WHERE game_id = ?1")?
            .query_row([game_id], |r| r.get(0))
            .optional()?
            .flatten();
        let Some(target) = target else {
            return Ok(Vec::new());
        };

        let hit: Option<(String, f64, String)> = conn
            .prepare_cached(
                "SELECT shop, price, currency FROM price_snapshot
                 WHERE game_id = ?1 AND country = ?2 AND price <= ?3
                 ORDER BY price ASC LIMIT 1",
            )?
            .query_row(params![game_id, country, target], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let Some((shop, price, currency)) = hit else {
            return Ok(Vec::new());
        };

        let inserted = conn
            .prepare_cached(
                "INSERT OR IGNORE INTO alert_log (game_id, shop, price, notified_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![game_id, shop, price, now()])?;

        if inserted == 0 {
            // Already announced at this price.
            return Ok(Vec::new());
        }

        let name: String = conn
            .prepare_cached("SELECT name FROM game WHERE id = ?1")?
            .query_row([game_id], |r| r.get(0))?;

        Ok(vec![PriceAlert {
            game_id,
            name,
            shop,
            price,
            currency,
            target,
        }])
    })
}

/// Discounted wishlist games, best deal per game, ranked by how good it is.
#[tauri::command]
pub fn list_active_deals(state: State<'_, AppState>) -> Result<Vec<DealRow>> {
    let (country, shops) = state.db.with(|conn| {
        let s = settings::read(conn)?;
        Ok((s.country, s.enabled_shops))
    })?;

    state.db.with(|conn| {
        // Every discounted offer, then narrowed and reduced in Rust. Picking
        // the minimum in SQL first would drop a game whose cheapest price
        // happens to be at a shop the user does not buy from.
        let mut stmt = conn.prepare_cached(
            "SELECT e.id, g.id, g.name, g.cover_image_id, s.shop, s.currency, s.price,
                    s.regular, s.cut, s.url, s.sale_expiry, e.target_price,
                    (SELECT price FROM price_low l
                      WHERE l.game_id = g.id AND l.country = s.country AND l.scope = 'all')
             FROM entry e
             JOIN game g ON g.id = e.game_id
             JOIN price_snapshot s ON s.game_id = g.id AND s.country = ?1
             WHERE e.owned = 0 AND e.status = 'want' AND s.cut > 0",
        )?;

        let rows = stmt.query_map([&country], |r| {
            let price: f64 = r.get(6)?;
            let target: Option<f64> = r.get(11)?;
            let all_time: Option<f64> = r.get(12)?;
            Ok(DealRow {
                entry_id: r.get(0)?,
                game_id: r.get(1)?,
                name: r.get(2)?,
                cover_image_id: r.get(3)?,
                shop: r.get(4)?,
                currency: r.get(5)?,
                price,
                regular: r.get(7)?,
                cut: r.get(8)?,
                url: r.get(9)?,
                sale_expiry: r.get(10)?,
                all_time_low: all_time,
                vs_all_time_low: all_time.filter(|l| *l > 0.0).map(|l| price / l),
                target_price: target,
                meets_target: target.map(|t| price <= t).unwrap_or(false),
            })
        })?;

        let mut best: std::collections::HashMap<i64, DealRow> = std::collections::HashMap::new();
        for row in rows {
            let row = row?;
            if !shops.is_empty() && !shops.iter().any(|s| s == &row.shop) {
                continue;
            }
            best.entry(row.game_id)
                .and_modify(|held| {
                    if row.price < held.price {
                        *held = row.clone();
                    }
                })
                .or_insert(row);
        }

        let mut out: Vec<DealRow> = best.into_values().collect();
        out.sort_by_key(|d| std::cmp::Reverse(d.cut));
        Ok(out)
    })
}

/// Record a price by hand — the fallback for stores with no usable API
/// (Xbox today, PlayStation whenever Sony rotates its query hashes).
#[tauri::command]
pub fn set_manual_price(
    state: State<'_, AppState>,
    igdb_id: i64,
    platform_family: String,
    shop: String,
    price: f64,
    currency: String,
    url: Option<String>,
) -> Result<PriceOverview> {
    let country = state.db.with(|conn| Ok(settings::read(conn)?.country))?;
    let dir = state.env_path.parent().map(|p| p.to_path_buf());

    state.db.with(|conn| {
        conn.prepare_cached(
            "INSERT INTO price_snapshot
               (game_id, shop, country, platform_family, currency, price, regular, cut,
                url, drm, sale_expiry, source, fetched_at)
             VALUES (?1,?2,?3,?4,?5,?6,NULL,0,?7,NULL,NULL,'manual',?8)
             ON CONFLICT (game_id, shop, country) DO UPDATE SET
               platform_family = excluded.platform_family, currency = excluded.currency,
               price = excluded.price, url = excluded.url, source = 'manual',
               fetched_at = excluded.fetched_at",
        )?
        .execute(params![
            igdb_id,
            shop,
            country,
            platform_family,
            currency,
            price,
            url,
            now()
        ])?;
        let s = settings::read(conn)?;
        pricing::overview(
            conn,
            igdb_id,
            &s.country,
            &s.enabled_shops,
            dir.as_deref(),
            None,
        )
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

    /// One game and its entry. Returns the entry id.
    fn entry(conn: &Connection, game_id: i64, owned: bool, status: &str) -> i64 {
        conn.execute(
            "INSERT INTO game (id, name, metadata_fetched) VALUES (?1, ?2, 0)",
            params![game_id, format!("Game {game_id}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entry (game_id, owned, status, priority, added_at, updated_at)
             VALUES (?1, ?2, ?3, ?1, 0, 0)",
            params![game_id, owned as i64, status],
        )
        .unwrap();
        conn.query_row("SELECT id FROM entry WHERE game_id = ?1", [game_id], |r| {
            r.get(0)
        })
        .unwrap()
    }

    fn queue(conn: &Connection, entry_id: i64, position: i64) {
        conn.execute(
            "INSERT INTO queue (entry_id, position, added_at) VALUES (?1, ?2, 0)",
            params![entry_id, position],
        )
        .unwrap();
    }

    #[test]
    fn a_queued_game_is_watched_even_when_it_is_no_longer_wanted() {
        let conn = db();
        entry(&conn, 1, false, "want"); // plain wishlist
        let dropped = entry(&conn, 2, false, "dropped"); // queued, but not "want"
        queue(&conn, dropped, 0);

        // Without the queue clause this game would never get a price, and its
        // row would sit blank forever.
        assert_eq!(watch_targets(&conn).unwrap(), vec![1, 2]);
    }

    #[test]
    fn owned_games_are_never_watched_even_when_queued() {
        let conn = db();
        let owned = entry(&conn, 1, true, "playing");
        queue(&conn, owned, 0);
        entry(&conn, 2, false, "want");

        // Owned rows show ownership instead of a price, so spending requests
        // on them would buy nothing.
        assert_eq!(watch_targets(&conn).unwrap(), vec![2]);
        assert!(queue_targets(&conn).unwrap().is_empty());
    }

    #[test]
    fn the_queue_refresh_follows_the_list_order() {
        let conn = db();
        let a = entry(&conn, 1, false, "want");
        let b = entry(&conn, 2, false, "want");
        queue(&conn, b, 0);
        queue(&conn, a, 1);

        // Whatever is at the top of Play Next is what the user is deciding
        // about, so it gets its price first.
        assert_eq!(queue_targets(&conn).unwrap(), vec![2, 1]);
    }
}
