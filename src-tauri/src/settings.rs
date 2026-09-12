use serde::{Deserialize, Serialize};

use crate::db::{get_setting, set_setting};
use crate::error::Result;

const KEY: &str = "settings";

/// Names exactly as IsThereAnyDeal returns them, plus the PlayStation store
/// that Phase 2b will fill in.
pub const DEFAULT_SHOPS: [&str; 5] = [
    "Steam",
    "GOG",
    "Epic Game Store",
    "Microsoft Store",
    "PlayStation Store",
];

/// User-facing app settings, stored as one JSON document in the `setting`
/// table. `#[serde(default)]` means a document written by an older build
/// still deserialises when new fields are added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// ISO country code — prices are region-specific on every source.
    pub country: String,
    pub currency: String,
    pub theme: String,
    /// How often the background refresher checks wishlist prices (Phase 2).
    pub refresh_interval_hours: u32,
    pub notifications_enabled: bool,
    /// Shops to show prices for. Empty means "every shop the source returns",
    /// which is rarely what anyone wants — twenty grey-market keysellers bury
    /// the four or five stores a person actually buys from.
    pub enabled_shops: Vec<String>,
    pub track_playstation: bool,
    /// Off by default: rides on an undocumented Nintendo endpoint.
    pub track_nintendo: bool,
    /// Resolved SteamID64, remembered after the first import.
    pub steam_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            country: "DE".into(),
            currency: "EUR".into(),
            theme: "dark".into(),
            refresh_interval_hours: 6,
            notifications_enabled: true,
            enabled_shops: DEFAULT_SHOPS.iter().map(|s| (*s).to_string()).collect(),
            track_playstation: true,
            track_nintendo: false,
            steam_id: None,
        }
    }
}

/// Partial update. Absent fields are left untouched.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub country: Option<String>,
    pub currency: Option<String>,
    pub theme: Option<String>,
    pub refresh_interval_hours: Option<u32>,
    pub notifications_enabled: Option<bool>,
    pub enabled_shops: Option<Vec<String>>,
    pub track_playstation: Option<bool>,
    pub track_nintendo: Option<bool>,
    /// Doubly wrapped so `null` can clear a stored id, rather than reading as
    /// "leave it alone" the way a plain Option would.
    #[serde(deserialize_with = "crate::db::models::double_option")]
    pub steam_id: Option<Option<String>>,
}

impl SettingsPatch {
    fn apply_to(self, s: &mut Settings) {
        if let Some(v) = self.country {
            s.country = v.trim().to_uppercase();
        }
        if let Some(v) = self.currency {
            s.currency = v.trim().to_uppercase();
        }
        if let Some(v) = self.theme {
            s.theme = v;
        }
        if let Some(v) = self.refresh_interval_hours {
            s.refresh_interval_hours = v.clamp(1, 168);
        }
        if let Some(v) = self.notifications_enabled {
            s.notifications_enabled = v;
        }
        if let Some(v) = self.enabled_shops {
            s.enabled_shops = v;
        }
        if let Some(v) = self.track_playstation {
            s.track_playstation = v;
        }
        if let Some(v) = self.track_nintendo {
            s.track_nintendo = v;
        }
        if let Some(v) = self.steam_id {
            s.steam_id = v.filter(|id| !id.trim().is_empty());
        }
    }
}

pub fn read(conn: &rusqlite::Connection) -> Result<Settings> {
    let mut settings = match get_setting(conn, KEY)? {
        Some(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("settings were unreadable ({e}); falling back to defaults");
            Settings::default()
        }),
        None => Settings::default(),
    };

    // An empty list means "not chosen yet", not "show every shop". This also
    // upgrades installs written before the whitelist existed, whose stored
    // settings hold an empty array.
    if settings.enabled_shops.is_empty() {
        settings.enabled_shops = DEFAULT_SHOPS.iter().map(|s| (*s).to_string()).collect();
    }
    settings
        .enabled_shops
        .retain(|s| !s.trim().is_empty());

    Ok(settings)
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
    fn a_fresh_install_gets_the_default_shop_whitelist() {
        let conn = db();
        let s = read(&conn).unwrap();
        assert!(s.enabled_shops.contains(&"Steam".to_string()));
        assert!(s.enabled_shops.contains(&"GOG".to_string()));
        assert!(s.enabled_shops.contains(&"Epic Game Store".to_string()));
        assert_eq!(s.enabled_shops.len(), DEFAULT_SHOPS.len());
        assert_eq!(s.currency, "EUR");
    }

    #[test]
    fn settings_written_before_the_whitelist_existed_are_upgraded() {
        let conn = db();
        // Exactly what an older build stored.
        set_setting(
            &conn,
            KEY,
            r#"{"country":"DE","currency":"EUR","theme":"dark","refreshIntervalHours":6,
                "notificationsEnabled":true,"enabledShops":[],
                "trackPlaystation":true,"trackNintendo":false}"#,
        )
        .unwrap();

        let s = read(&conn).unwrap();
        assert_eq!(
            s.enabled_shops.len(),
            DEFAULT_SHOPS.len(),
            "an empty list must mean 'not chosen', not 'every keyseller'"
        );
    }

    #[test]
    fn a_stored_steam_id_can_be_cleared_again() {
        let conn = db();
        patch(&conn, serde_json::from_str(r#"{"steamId":"76561197960287930"}"#).unwrap()).unwrap();
        assert_eq!(read(&conn).unwrap().steam_id.as_deref(), Some("76561197960287930"));

        patch(&conn, serde_json::from_str(r#"{"steamId":null}"#).unwrap()).unwrap();
        assert_eq!(read(&conn).unwrap().steam_id, None, "null must clear it");

        // An unrelated patch must leave it alone.
        patch(&conn, serde_json::from_str(r#"{"steamId":"123"}"#).unwrap()).unwrap();
        patch(&conn, serde_json::from_str(r#"{"country":"AT"}"#).unwrap()).unwrap();
        assert_eq!(read(&conn).unwrap().steam_id.as_deref(), Some("123"));
    }

    #[test]
    fn an_explicit_choice_is_respected() {
        let conn = db();
        patch(
            &conn,
            serde_json::from_str(r#"{"enabledShops":["Steam","GOG"]}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(read(&conn).unwrap().enabled_shops, vec!["Steam", "GOG"]);
    }
}

pub fn write(conn: &rusqlite::Connection, s: &Settings) -> Result<()> {
    set_setting(conn, KEY, &serde_json::to_string(s)?)
}

pub fn patch(conn: &rusqlite::Connection, p: SettingsPatch) -> Result<Settings> {
    let mut s = read(conn)?;
    p.apply_to(&mut s);
    write(conn, &s)?;
    Ok(s)
}
