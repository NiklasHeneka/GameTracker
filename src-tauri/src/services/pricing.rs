use rusqlite::{params, Connection, OptionalExtension};

use crate::clients::{cheapshark, itad};
use crate::db::now;
use crate::db::price_models::*;
use crate::error::Result;

/// Anything older than this is refetched on demand.
pub const FRESH_FOR_SECS: i64 = 6 * 3600;

// ── Persistence ──────────────────────────────────────────────────────────

pub fn replace_snapshots(
    conn: &Connection,
    game_id: i64,
    country: &str,
    rows: &[PriceRow],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // Replace the PC rows wholesale so a shop that stopped carrying the game
    // vanishes rather than lingering at a stale price. PlayStation and manual
    // entries come from elsewhere and must survive.
    tx.execute(
        "DELETE FROM price_snapshot
         WHERE game_id = ?1 AND country = ?2 AND source IN ('itad', 'cheapshark')",
        params![game_id, country],
    )?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO price_snapshot
               (game_id, shop, country, platform_family, currency, price, regular,
                cut, url, drm, sale_expiry, source, fetched_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT (game_id, shop, country) DO UPDATE SET
               platform_family = excluded.platform_family, currency = excluded.currency,
               price = excluded.price, regular = excluded.regular, cut = excluded.cut,
               url = excluded.url, drm = excluded.drm, sale_expiry = excluded.sale_expiry,
               source = excluded.source, fetched_at = excluded.fetched_at",
        )?;
        let ts = now();
        for r in rows {
            stmt.execute(params![
                game_id,
                r.shop,
                country,
                r.platform_family,
                r.currency,
                r.price,
                r.regular,
                r.cut,
                r.url,
                Option::<String>::None,
                r.sale_expiry,
                r.source,
                ts
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Record what a wishlist game cost when tracking began, once.
///
/// Nothing else can reconstruct this later: by the time the game is bought,
/// the old price is gone. It is the baseline for "what waiting saved".
pub fn stamp_price_at_add(conn: &Connection, game_id: i64, country: &str) -> Result<()> {
    conn.prepare_cached(
        "UPDATE entry
            SET price_at_add = (SELECT MIN(price) FROM price_snapshot s
                                 WHERE s.game_id = entry.game_id AND s.country = ?2)
          WHERE game_id = ?1
            AND price_at_add IS NULL
            AND EXISTS (SELECT 1 FROM price_snapshot s
                         WHERE s.game_id = entry.game_id AND s.country = ?2)",
    )?
    .execute(params![game_id, country])?;
    Ok(())
}

/// Insert or update a single offer, leaving every other row alone.
pub fn upsert_snapshot(
    conn: &Connection,
    game_id: i64,
    country: &str,
    row: &PriceRow,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO price_snapshot
           (game_id, shop, country, platform_family, currency, price, regular,
            cut, url, drm, sale_expiry, source, fetched_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10,?11,?12)
         ON CONFLICT (game_id, shop, country) DO UPDATE SET
           platform_family = excluded.platform_family, currency = excluded.currency,
           price = excluded.price, regular = excluded.regular, cut = excluded.cut,
           url = excluded.url, sale_expiry = excluded.sale_expiry,
           source = excluded.source, fetched_at = excluded.fetched_at",
    )?
    .execute(params![
        game_id,
        row.shop,
        country,
        row.platform_family,
        row.currency,
        row.price,
        row.regular,
        row.cut,
        row.url,
        row.sale_expiry,
        row.source,
        now()
    ])?;
    Ok(())
}

pub fn save_low(conn: &Connection, game_id: i64, country: &str, low: &PriceLow) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO price_low (game_id, scope, shop, country, currency, price, cut,
                                occurred_at, fetched_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT (game_id, scope, shop, country) DO UPDATE SET
           currency = excluded.currency, price = excluded.price, cut = excluded.cut,
           occurred_at = excluded.occurred_at, fetched_at = excluded.fetched_at",
    )?
    .execute(params![
        game_id,
        low.scope,
        low.shop.clone().unwrap_or_default(),
        country,
        low.currency,
        low.price,
        low.cut,
        low.occurred_at,
        now()
    ])?;
    Ok(())
}

/// One recorded price change: timestamp, shop, currency, price, regular
/// price, and the discount percentage.
pub type HistoryRow = (i64, String, String, f64, Option<f64>, i64);

pub fn save_history(
    conn: &Connection,
    game_id: i64,
    country: &str,
    entries: &[HistoryRow],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO price_history (game_id, country, ts, shop, currency, price, regular, cut)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT (game_id, country, ts, shop) DO UPDATE SET
               price = excluded.price, regular = excluded.regular, cut = excluded.cut",
        )?;
        for (ts, shop, currency, price, regular, cut) in entries {
            stmt.execute(params![
                game_id, country, ts, shop, currency, price, regular, cut
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ── Reading back ─────────────────────────────────────────────────────────

fn read_rows(
    conn: &Connection,
    game_id: i64,
    country: &str,
    shops: &[String],
) -> Result<Vec<PriceRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT shop, platform_family, currency, price, regular, cut, url, sale_expiry, source
         FROM price_snapshot WHERE game_id = ?1 AND country = ?2
         ORDER BY price ASC",
    )?;
    let rows = stmt.query_map(params![game_id, country], |r| {
        Ok(PriceRow {
            shop: r.get(0)?,
            platform_family: r.get(1)?,
            currency: r.get(2)?,
            price: r.get(3)?,
            regular: r.get(4)?,
            cut: r.get(5)?,
            url: r.get(6)?,
            sale_expiry: r.get(7)?,
            source: r.get(8)?,
            is_all_time_low: false,
        })
    })?;
    let all: Vec<PriceRow> = rows.collect::<std::result::Result<_, _>>()?;

    // An empty whitelist means "no filtering"; otherwise keep only the stores
    // the user buys from. Manual entries always survive — the user typed them.
    Ok(if shops.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|r| r.source == "manual" || shops.iter().any(|s| s == &r.shop))
            .collect()
    })
}

fn read_lows(conn: &Connection, game_id: i64, country: &str) -> Result<Vec<PriceLow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT scope, shop, currency, price, cut, occurred_at
         FROM price_low WHERE game_id = ?1 AND country = ?2",
    )?;
    let rows = stmt.query_map(params![game_id, country], |r| {
        let shop: String = r.get(1)?;
        Ok(PriceLow {
            scope: r.get(0)?,
            shop: (!shop.is_empty()).then_some(shop),
            currency: r.get(2)?,
            price: r.get(3)?,
            cut: r.get(4)?,
            occurred_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Collapse the per-shop log into one point per day: the *best* price
/// available that day.
///
/// ITAD reports every shop separately, so the raw series flips in and out of
/// discount as twenty stores change independently — which made the sparkline
/// zigzag and the cadence estimate report a sale every day. What a buyer cares
/// about is the cheapest offer on any given day.
///
/// The bare `cut` column is safe here: with exactly one `MIN()` aggregate,
/// SQLite takes the other columns from the row that produced the minimum.
fn read_history(conn: &Connection, game_id: i64, country: &str) -> Result<Vec<HistoryPoint>> {
    let mut stmt = conn.prepare_cached(
        "SELECT (ts / 86400) * 86400 AS day, MIN(price), cut
         FROM price_history
         WHERE game_id = ?1 AND country = ?2
         GROUP BY ts / 86400
         ORDER BY day ASC",
    )?;
    let rows = stmt.query_map(params![game_id, country], |r| {
        Ok(HistoryPoint {
            ts: r.get(0)?,
            price: r.get(1)?,
            cut: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn fetched_at(conn: &Connection, game_id: i64, country: &str) -> Result<Option<i64>> {
    Ok(conn
        .prepare_cached(
            "SELECT MAX(fetched_at) FROM price_snapshot WHERE game_id = ?1 AND country = ?2",
        )?
        .query_row(params![game_id, country], |r| r.get::<_, Option<i64>>(0))
        .optional()?
        .flatten())
}

// ── Derived views ────────────────────────────────────────────────────────

/// The most recent discount that has already ended.
pub fn last_sale(history: &[HistoryPoint], currency: &str) -> Option<LastSale> {
    // Newest first; skip anything still running (the current price is a
    // separate concern shown as a live deal).
    let mut discounts: Vec<&HistoryPoint> = history.iter().filter(|p| p.cut > 0).collect();
    discounts.sort_by_key(|p| std::cmp::Reverse(p.ts));
    let latest = discounts.first()?;
    Some(LastSale {
        price: latest.price,
        currency: currency.to_string(),
        cut: latest.cut,
        occurred_at: latest.ts,
        shop: None,
    })
}

/// Describe how often the game has been discounted over the observed window.
///
/// Needs at least a month of history before it says anything — a fortnight of
/// data cannot characterise a year.
pub fn discount_pattern(history: &[HistoryPoint]) -> Option<DiscountPattern> {
    if history.len() < 2 {
        return None;
    }

    let first = history.first()?.ts;
    let last = history.last()?.ts;
    let span_days = (last - first) / 86_400;
    if span_days < 30 {
        return None;
    }

    let days_discounted = history.iter().filter(|p| p.cut > 0).count() as i64;
    let days_observed = history.len() as i64;
    let deepest_cut = history.iter().map(|p| p.cut).max().unwrap_or(0);

    Some(DiscountPattern {
        days_observed,
        days_discounted,
        share_percent: (days_discounted * 100) / days_observed.max(1),
        deepest_cut,
    })
}

pub fn overview(
    conn: &Connection,
    game_id: i64,
    country: &str,
    shops: &[String],
    config_dir: Option<&std::path::Path>,
    note: Option<String>,
) -> Result<PriceOverview> {
    let mut rows = read_rows(conn, game_id, country, shops)?;
    let lows = read_lows(conn, game_id, country)?;
    let history = read_history(conn, game_id, country)?;

    // Flag any offer that matches the all-time low — the single most useful
    // buy/wait signal on the panel.
    if let Some(all_time) = lows.iter().find(|l| l.scope == "all") {
        for row in rows.iter_mut() {
            // Compare in cents to dodge float equality.
            if (row.price * 100.0).round() as i64 <= (all_time.price * 100.0).round() as i64 {
                row.is_all_time_low = true;
            }
        }
    }

    let currency = rows
        .first()
        .map(|r| r.currency.clone())
        .or_else(|| lows.first().map(|l| l.currency.clone()))
        .unwrap_or_else(|| "EUR".into());

    let mut groups: Vec<PriceGroup> = Vec::new();
    for family in ["pc", "playstation", "nintendo", "xbox"] {
        let group: Vec<PriceRow> = rows
            .iter()
            .filter(|r| r.platform_family == family)
            .cloned()
            .collect();
        if !group.is_empty() {
            groups.push(PriceGroup {
                family: family.into(),
                rows: group,
            });
        }
    }

    let at = fetched_at(conn, game_id, country)?;

    Ok(PriceOverview {
        game_id,
        country: country.to_string(),
        groups,
        last_sale: last_sale(&history, &currency),
        pattern: discount_pattern(&history),
        lows,
        history,
        sale_outlook: crate::services::sale_calendar::outlook(
            conn, game_id, country, shops, config_dir,
        )?,
        fetched_at: at,
        stale: at.map(|t| now() - t > FRESH_FOR_SECS).unwrap_or(true),
        note,
    })
}

// ── Mapping from the wire ────────────────────────────────────────────────

pub fn rows_from_itad(deals: &[itad::Deal]) -> Vec<PriceRow> {
    deals
        .iter()
        .map(|d| PriceRow {
            shop: d.shop.name.clone(),
            platform_family: "pc".into(),
            currency: d.price.currency.clone(),
            price: d.price.amount,
            regular: d.regular.as_ref().map(|m| m.amount),
            cut: d.cut,
            url: d.url.clone(),
            sale_expiry: d.expiry.as_deref().and_then(itad::parse_timestamp),
            source: "itad".into(),
            is_all_time_low: false,
        })
        .collect()
}

pub fn rows_from_cheapshark(
    detail: &cheapshark::GameDetail,
    store_names: &std::collections::HashMap<String, String>,
) -> Vec<PriceRow> {
    detail
        .deals
        .iter()
        .filter_map(|d| {
            let price: f64 = d.price.parse().ok()?;
            let regular: Option<f64> = d.retail_price.parse().ok();
            let savings: f64 = d.savings.parse().unwrap_or(0.0);
            Some(PriceRow {
                shop: store_names
                    .get(&d.store_id)
                    .cloned()
                    .unwrap_or_else(|| format!("Store {}", d.store_id)),
                platform_family: "pc".into(),
                currency: cheapshark::CURRENCY.into(),
                price,
                regular,
                cut: savings.round() as i64,
                url: Some(format!(
                    "https://www.cheapshark.com/redirect?dealID={}",
                    d.deal_id
                )),
                // CheapShark does not publish when a sale ends.
                sale_expiry: None,
                source: "cheapshark".into(),
                is_all_time_low: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn point(days_ago: i64, price: f64, cut: i64) -> HistoryPoint {
        HistoryPoint {
            ts: now() - days_ago * DAY,
            price,
            cut,
        }
    }

    #[test]
    fn last_sale_picks_the_most_recent_discount_not_the_deepest() {
        let history = vec![
            point(300, 4.99, 75), // deepest, but old
            point(90, 7.49, 50),  // most recent discount
            point(200, 9.99, 33),
            point(10, 14.99, 0), // back to full price
        ];
        let last = last_sale(&history, "EUR").expect("a past sale");
        assert_eq!(last.cut, 50);
        assert!((last.price - 7.49).abs() < f64::EPSILON);
    }

    #[test]
    fn last_sale_is_none_when_it_has_never_been_discounted() {
        let history = vec![point(100, 14.99, 0), point(10, 14.99, 0)];
        assert!(last_sale(&history, "EUR").is_none());
    }

    #[test]
    fn discount_share_reflects_how_much_of_the_window_was_on_sale() {
        // 100 days, discounted on 60 of them, deepest cut 80%.
        let mut history = Vec::new();
        for day in (0..100).rev() {
            let discounted = day % 10 < 6;
            history.push(point(
                day,
                if discounted { 5.0 } else { 15.0 },
                if discounted { 66 } else { 0 },
            ));
        }
        history.push(point(3, 3.0, 80));

        let p = discount_pattern(&history).expect("enough history");
        assert_eq!(p.share_percent, 60);
        assert_eq!(p.deepest_cut, 80);
        assert_eq!(p.days_discounted, 61);
    }

    #[test]
    fn a_short_window_says_nothing_rather_than_extrapolating() {
        // Two weeks cannot characterise a year.
        let history: Vec<_> = (0..14)
            .rev()
            .map(|d| point(d, 10.0, if d < 7 { 50 } else { 0 }))
            .collect();
        assert!(discount_pattern(&history).is_none());
        assert!(discount_pattern(&[]).is_none());
    }

    #[test]
    fn a_game_discounted_almost_always_reports_a_high_share() {
        // The real Disco Elysium shape: on sale somewhere most days.
        let history: Vec<_> = (0..600)
            .rev()
            .map(|d| point(d, 9.0, if d % 3 == 0 { 0 } else { 70 }))
            .collect();
        let p = discount_pattern(&history).unwrap();
        assert!(
            p.share_percent > 60,
            "expected a high share, got {}",
            p.share_percent
        );
    }

    fn seeded_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO game (id, name, metadata_fetched) VALUES (1, 'Test', 0)",
            [],
        )
        .unwrap();
        conn
    }

    /// The bug the live run caught: twenty shops changing prices independently
    /// made the merged series look like a sale every single day.
    #[test]
    fn history_collapses_many_shops_into_one_best_price_per_day() {
        let conn = seeded_db();
        let base = now() - 40 * DAY;

        // Three shops, same three days. Only GOG is ever discounted.
        let rows = [
            (base, "Steam", 14.99, 0),
            (base, "GOG", 14.79, 0),
            (base, "Humble", 15.99, 0),
            (base + DAY, "Steam", 14.99, 0),
            (base + DAY, "GOG", 7.39, 50),
            (base + DAY, "Humble", 15.99, 0),
            (base + 2 * DAY, "Steam", 14.99, 0),
            (base + 2 * DAY, "GOG", 14.79, 0),
        ];
        let entries: Vec<_> = rows
            .iter()
            .map(|(ts, shop, price, cut)| {
                (*ts, shop.to_string(), "EUR".to_string(), *price, None, *cut)
            })
            .collect();
        save_history(&conn, 1, "DE", &entries).unwrap();

        let history = read_history(&conn, 1, "DE").unwrap();
        assert_eq!(
            history.len(),
            3,
            "one point per day, not one per shop per day"
        );
        assert!(
            (history[0].price - 14.79).abs() < 1e-9,
            "day one takes the cheapest shop"
        );
        assert!((history[1].price - 7.39).abs() < 1e-9);
        assert_eq!(
            history[1].cut, 50,
            "cut must come from the row that was cheapest"
        );
        assert_eq!(history[2].cut, 0);
    }

    #[test]
    fn cheapshark_rows_are_labelled_usd_and_carry_no_expiry() {
        let detail: crate::clients::cheapshark::GameDetail = serde_json::from_str(
            r#"{"cheapestPriceEver":{"price":"2.99","date":1712840581},
                "deals":[{"storeID":"1","price":"7.49","retailPrice":"14.99",
                          "savings":"50.030020","dealID":"abc"}]}"#,
        )
        .unwrap();
        let mut names = std::collections::HashMap::new();
        names.insert("1".to_string(), "Steam".to_string());

        let rows = rows_from_cheapshark(&detail, &names);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].shop, "Steam");
        assert_eq!(
            rows[0].currency, "USD",
            "CheapShark has no region parameter"
        );
        assert_eq!(
            rows[0].cut, 50,
            "savings string must round to a whole percent"
        );
        assert!(rows[0].sale_expiry.is_none());
        assert_eq!(rows[0].source, "cheapshark");
    }

    #[test]
    fn an_unknown_cheapshark_store_id_still_produces_a_row() {
        let detail: crate::clients::cheapshark::GameDetail = serde_json::from_str(
            r#"{"deals":[{"storeID":"99","price":"1.00","retailPrice":"2.00",
                          "savings":"50.0","dealID":"x"}]}"#,
        )
        .unwrap();
        let rows = rows_from_cheapshark(&detail, &std::collections::HashMap::new());
        assert_eq!(rows[0].shop, "Store 99", "must not drop the offer entirely");
    }

    #[test]
    fn itad_rows_carry_expiry_and_the_local_currency() {
        let deals: Vec<crate::clients::itad::Deal> = serde_json::from_str(
            r#"[{"shop":{"name":"GOG"},
                 "price":{"amount":7.39,"currency":"EUR"},
                 "regular":{"amount":14.79,"currency":"EUR"},
                 "cut":50,"url":"https://gog.com/x","drm":[],
                 "expiry":"2026-09-10T17:00:00+02:00"},
                {"shop":{"name":"Steam"},
                 "price":{"amount":14.79,"currency":"EUR"},
                 "regular":{"amount":14.79,"currency":"EUR"},
                 "cut":0,"url":null,"drm":[],"expiry":null}]"#,
        )
        .unwrap();
        let rows = rows_from_itad(&deals);
        assert_eq!(rows[0].currency, "EUR");
        assert_eq!(rows[0].cut, 50);
        assert_eq!(rows[0].sale_expiry, Some(1789052400)); // 2026-09-10T15:00Z
                                                           // Roughly half of live deals have no expiry; that must stay optional.
        assert!(rows[1].sale_expiry.is_none());
    }
}
