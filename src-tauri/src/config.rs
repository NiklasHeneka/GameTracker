use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::Result;

/// Environment variable names. Keep these in sync with `.env.example`.
pub const KEYS: &[(&str, &str, bool)] = &[
    // (env var, human label, required for core functionality)
    ("IGDB_CLIENT_ID", "IGDB / Twitch Client ID", true),
    ("IGDB_CLIENT_SECRET", "IGDB / Twitch Client Secret", true),
    ("ITAD_API_KEY", "IsThereAnyDeal API key", false),
    ("STEAM_API_KEY", "Steam Web API key", false),
];

/// Credentials read from the environment. Deliberately **not** `Serialize`:
/// values must never cross the IPC boundary into the webview. The frontend
/// only ever sees [`ConfigStatus`].
#[derive(Debug, Default, Clone)]
pub struct Credentials {
    pub igdb_client_id: Option<String>,
    pub igdb_client_secret: Option<String>,
    pub itad_api_key: Option<String>,
    pub steam_api_key: Option<String>,
}

impl Credentials {
    /// Read the current process environment. Call after `dotenvy` has run.
    pub fn from_env() -> Self {
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            igdb_client_id: get("IGDB_CLIENT_ID"),
            igdb_client_secret: get("IGDB_CLIENT_SECRET"),
            itad_api_key: get("ITAD_API_KEY"),
            steam_api_key: get("STEAM_API_KEY"),
        }
    }
}

/// What the frontend is allowed to know: which credentials exist, never what
/// they are, plus where to go to edit them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
    /// Absolute path of the `.env` the app read (or would create).
    pub env_path: String,
    pub env_exists: bool,
    pub keys: Vec<KeyStatus>,
    /// True when everything needed for metadata (IGDB) is present.
    pub ready: bool,
    /// When Sony last refused the PlayStation query hash, if it currently is.
    /// Prices still arrive through the store-page fallback, so without this
    /// the rotation would go unnoticed. Filled in by the command, which has
    /// the database; this module only knows about `.env`.
    pub ps_hash_rejected_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub name: String,
    pub label: String,
    pub required: bool,
    pub present: bool,
    /// e.g. `abcd…wxyz` — enough to confirm which key is loaded, not enough to use it.
    pub hint: Option<String>,
}

fn hint(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len().max(4));
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// Resolve which `.env` to use, in priority order:
///   1. `$GAMETRACKER_ENV_FILE` — explicit override
///   2. the project root during development (walk up from the cwd)
///   3. `<app config dir>/.env` — the home for an installed build
pub fn resolve_env_path(app: &AppHandle) -> PathBuf {
    if let Ok(explicit) = std::env::var("GAMETRACKER_ENV_FILE") {
        if !explicit.trim().is_empty() {
            return PathBuf::from(explicit);
        }
    }

    if cfg!(debug_assertions) {
        // `tauri dev` runs with the cwd at src-tauri/, so the project root is
        // one level up. Walk a little further in case that ever changes.
        if let Ok(cwd) = std::env::current_dir() {
            let mut dir: Option<&Path> = Some(cwd.as_path());
            for _ in 0..3 {
                let Some(d) = dir else { break };
                let candidate = d.join(".env");
                if candidate.is_file() {
                    return candidate;
                }
                // Anchor on the project root even when .env has not been created yet.
                if d.join("package.json").is_file() {
                    return candidate;
                }
                dir = d.parent();
            }
        }
    }

    app.path()
        .app_config_dir()
        .map(|d| d.join(".env"))
        .unwrap_or_else(|_| PathBuf::from(".env"))
}

const TEMPLATE: &str = r#"# GameTracker credentials — this file is read by the Rust backend only.
# It is git-ignored and never bundled into the frontend.
#
# IGDB (required) — register an application at https://dev.twitch.tv/console/apps
# Set the OAuth redirect URL to http://localhost and copy the Client ID + Secret.
IGDB_CLIENT_ID=
IGDB_CLIENT_SECRET=

# IsThereAnyDeal (optional, Phase 2) — https://isthereanydeal.com/apps/my/
# Without it the app falls back to CheapShark, which needs no key.
ITAD_API_KEY=

# Steam Web API (optional, Phase 3 — library import) — https://steamcommunity.com/dev/apikey
# Your Steam profile and game details must be public for the import to see them.
STEAM_API_KEY=
"#;

/// Create the `.env` from a template if it does not exist yet, with
/// owner-only permissions on unix.
fn ensure_env_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, TEMPLATE)?;
    restrict_permissions(path)?;
    log::info!("created credentials template at {}", path.display());
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    // Windows inherits the user profile ACL, which is already owner-scoped.
    Ok(())
}

/// Load credentials at startup. Real process environment variables win over
/// the file, so `IGDB_CLIENT_ID=… npm run app` works for one-off overrides.
pub fn load(app: &AppHandle) -> Result<(Credentials, PathBuf)> {
    let path = resolve_env_path(app);
    ensure_env_file(&path)?;

    match dotenvy::from_path(&path) {
        Ok(()) => log::info!("loaded credentials from {}", path.display()),
        Err(e) => log::warn!("could not read {}: {e}", path.display()),
    }

    Ok((Credentials::from_env(), path))
}

pub fn status(env_path: &Path) -> ConfigStatus {
    let keys: Vec<KeyStatus> = KEYS
        .iter()
        .map(|(name, label, required)| {
            let value = std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            KeyStatus {
                name: (*name).to_string(),
                label: (*label).to_string(),
                required: *required,
                present: value.is_some(),
                hint: value.as_deref().map(hint),
            }
        })
        .collect();

    let ready = keys.iter().filter(|k| k.required).all(|k| k.present);

    ConfigStatus {
        env_path: env_path.display().to_string(),
        env_exists: env_path.is_file(),
        keys,
        ready,
        ps_hash_rejected_at: None,
    }
}
