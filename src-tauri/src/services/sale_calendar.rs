use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::clients::itad::parse_timestamp;
use crate::db::now;
use crate::db::price_models::{PastEventPrice, StoreSaleOutlook};
use crate::error::Result;

/// Shipped with the app; a copy placed next to `.env` takes precedence so the
/// user can correct dates without waiting for a release.
const BUNDLED: &str = include_str!("../../sale-calendar.json");

#[derive(Debug, Deserialize)]
struct Calendar {
    shops: HashMap<String, Vec<Event>>,
}

#[derive(Debug, Clone, Deserialize)]
struct Event {
    name: String,
    start: String,
    end: String,
    /// True only when the store has published the dates.
    #[serde(default)]
    confirmed: bool,
}

struct Window {
    name: String,
    start: i64,
    end: i64,
    confirmed: bool,
}

fn parse_day(date: &str, end_of_day: bool) -> Option<i64> {
    let ts = parse_timestamp(&format!("{date}T00:00:00Z"))?;
    // Events are inclusive, so an end date covers the whole day.
    Some(if end_of_day { ts + 86_399 } else { ts })
}

/// Midday UTC on the same calendar date.
///
/// Timestamps sent to the interface are formatted in the viewer's local time.
/// Midnight or 23:59 UTC lands on the neighbouring day for anyone east or west
/// of UTC — an 8 October end date was rendering as 9 October in Berlin — while
/// midday reads correctly across every real-world offset.
fn midday(ts: i64) -> i64 {
    ts - ts.rem_euclid(86_400) + 43_200
}

fn load(override_dir: Option<&Path>) -> Calendar {
    if let Some(dir) = override_dir {
        let path = dir.join("sale-calendar.json");
        if path.is_file() {
            match std::fs::read_to_string(&path).map(|s| serde_json::from_str::<Calendar>(&s)) {
                Ok(Ok(cal)) => {
                    log::info!("using sale calendar from {}", path.display());
                    return cal;
                }
                Ok(Err(e)) => log::warn!("{} is not valid JSON ({e}); using the bundled calendar", path.display()),
                Err(e) => log::warn!("could not read {} ({e}); using the bundled calendar", path.display()),
            }
        }
    }
    serde_json::from_str(BUNDLED).expect("the bundled sale calendar must parse")
}

fn windows(cal: &Calendar, shop: &str) -> Vec<Window> {
    let mut out: Vec<Window> = cal
        .shops
        .get(shop)
        .map(|events| {
            events
                .iter()
                .filter_map(|e| {
                    Some(Window {
                        name: e.name.clone(),
                        start: parse_day(&e.start, false)?,
                        end: parse_day(&e.end, true)?,
                        confirmed: e.confirmed,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|w| w.start);
    out
}

/// Cheapest price this game reached at this shop during a past sale window.
fn price_during(
    conn: &Connection,
    game_id: i64,
    country: &str,
    shop: &str,
    start: i64,
    end: i64,
) -> Result<Option<(f64, i64, String)>> {
    Ok(conn
        .prepare_cached(
            "SELECT MIN(price), cut, currency FROM price_history
             WHERE game_id = ?1 AND country = ?2 AND shop = ?3 AND ts BETWEEN ?4 AND ?5",
        )?
        .query_row(params![game_id, country, shop, start, end], |r| {
            Ok(r.get::<_, Option<f64>>(0)?
                .map(|p| (p, r.get::<_, i64>(1).unwrap_or(0), r.get::<_, String>(2).unwrap_or_default())))
        })
        .optional()?
        .flatten())
}

/// For each shop the user buys from, the next known storewide sale and what
/// this game cost during the previous run of that same sale.
pub fn outlook(
    conn: &Connection,
    game_id: i64,
    country: &str,
    shops: &[String],
    config_dir: Option<&Path>,
) -> Result<Vec<StoreSaleOutlook>> {
    let cal = load(config_dir);
    let today = now();
    let mut out = Vec::new();

    for shop in shops {
        let events = windows(&cal, shop);

        // The next sale that has not finished yet.
        let Some(next) = events.iter().find(|w| w.end >= today) else {
            continue;
        };

        // The most recent finished run of that same sale — "was it discounted
        // last time, and for how much?"
        let previous = events
            .iter()
            .filter(|w| w.name == next.name && w.end < today)
            .max_by_key(|w| w.start);

        let last_event = match previous {
            Some(prev) => {
                let found = price_during(conn, game_id, country, shop, prev.start, prev.end)?;
                Some(PastEventPrice {
                    event: prev.name.clone(),
                    start: midday(prev.start),
                    end: midday(prev.end),
                    best_price: found.as_ref().map(|f| f.0),
                    best_cut: found.as_ref().map(|f| f.1),
                    currency: found
                        .as_ref()
                        .map(|f| f.2.clone())
                        .unwrap_or_else(|| "EUR".into()),
                    // Distinguishes "was not discounted" from "we have no data
                    // for that period", which mean very different things.
                    had_data: found.is_some(),
                })
            }
            None => None,
        };

        out.push(StoreSaleOutlook {
            shop: shop.clone(),
            event: next.name.clone(),
            next_start: midday(next.start),
            next_end: midday(next.end),
            confirmed: next.confirmed,
            live_now: next.start <= today,
            days_away: ((next.start - today) / 86_400).max(0),
            last_event,
        });
    }

    out.sort_by_key(|o| o.days_away);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn.execute("INSERT INTO game (id, name, metadata_fetched) VALUES (1,'T',0)", [])
            .unwrap();
        conn
    }

    #[test]
    fn the_bundled_calendar_parses_and_covers_every_default_shop() {
        let cal = load(None);
        for shop in crate::settings::DEFAULT_SHOPS {
            assert!(cal.shops.contains_key(shop), "no events for {shop}");
            assert!(!windows(&cal, shop).is_empty(), "{shop} has no parseable dates");
        }
    }

    #[test]
    fn every_event_has_an_end_after_its_start() {
        let cal = load(None);
        for shop in cal.shops.keys() {
            for w in windows(&cal, shop) {
                assert!(w.end > w.start, "{shop} {} ends before it starts", w.name);
                assert!(
                    w.end - w.start < 60 * 86_400,
                    "{shop} {} spans over two months — likely a typo",
                    w.name
                );
            }
        }
    }

    #[test]
    fn steam_has_a_confirmed_upcoming_sale() {
        let cal = load(None);
        let upcoming: Vec<_> = windows(&cal, "Steam")
            .into_iter()
            .filter(|w| w.end >= now())
            .collect();
        assert!(!upcoming.is_empty(), "the calendar has run out of Steam dates");
        assert!(
            upcoming.iter().any(|w| w.confirmed),
            "no confirmed Steam dates remain — the calendar needs updating"
        );
    }

    #[test]
    fn outlook_reports_what_the_game_cost_in_the_previous_run_of_the_same_sale() {
        let conn = db();
        let cal = load(None);

        // Find Steam's next sale and the previous run of that same event.
        let events = windows(&cal, "Steam");
        let next = events.iter().find(|w| w.end >= now()).unwrap();
        let prev = events
            .iter()
            .filter(|w| w.name == next.name && w.end < now())
            .max_by_key(|w| w.start)
            .expect("a previous run of the same Steam sale");

        // A discount inside that window, and an unrelated full price outside it.
        let inside = prev.start + 3600;
        let outside = prev.start - 30 * 86_400;
        crate::services::pricing::save_history(
            &conn,
            1,
            "DE",
            &[
                (inside, "Steam".into(), "EUR".into(), 9.99, Some(39.99), 75),
                (outside, "Steam".into(), "EUR".into(), 39.99, Some(39.99), 0),
            ],
        )
        .unwrap();

        let shops = vec!["Steam".to_string()];
        let out = outlook(&conn, 1, "DE", &shops, None).unwrap();
        let steam = out.iter().find(|o| o.shop == "Steam").expect("a Steam outlook");

        assert_eq!(steam.event, next.name);
        let last = steam.last_event.as_ref().expect("a previous event");
        assert!(last.had_data);
        assert_eq!(last.best_cut, Some(75));
        assert_eq!(last.best_price, Some(9.99), "must ignore prices outside the window");
    }

    #[test]
    fn no_recorded_price_is_distinguished_from_no_discount() {
        let conn = db();
        let shops = vec!["Steam".to_string()];
        let out = outlook(&conn, 1, "DE", &shops, None).unwrap();
        let steam = out.iter().find(|o| o.shop == "Steam").unwrap();

        // Nothing was ever stored, so we must not claim it was not discounted.
        if let Some(last) = &steam.last_event {
            assert!(!last.had_data);
            assert_eq!(last.best_price, None);
        }
    }

    /// An 8 October end date must not read as 9 October for a user in CEST.
    #[test]
    fn displayed_dates_land_on_the_intended_calendar_day() {
        let cal = load(None);
        for w in windows(&cal, "Steam") {
            for shown in [midday(w.start), midday(w.end)] {
                assert_eq!(shown.rem_euclid(86_400), 43_200, "not midday UTC");
                // Stable from UTC-11 to UTC+11, which covers every timezone
                // this app realistically runs in. The far Pacific (UTC+13/+14)
                // would still see the following day; not worth carrying dates
                // as strings for.
                let day = shown.div_euclid(86_400);
                for offset_hours in [-11, 0, 11] {
                    assert_eq!(
                        (shown + offset_hours * 3600).div_euclid(86_400),
                        day,
                        "date shifts at UTC{offset_hours:+}"
                    );
                }
            }
        }
    }

    #[test]
    fn only_requested_shops_appear() {
        let conn = db();
        let shops = vec!["GOG".to_string()];
        let out = outlook(&conn, 1, "DE", &shops, None).unwrap();
        assert!(out.iter().all(|o| o.shop == "GOG"));
    }
}
