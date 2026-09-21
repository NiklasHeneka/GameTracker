use tauri::State;

use crate::config::{self, ConfigStatus, Credentials};
use crate::error::{AppError, Result};
use crate::state::AppState;

/// Which credentials are present — never their values — plus anything else
/// the Settings screen needs to warn about.
#[tauri::command]
pub fn config_status(state: State<'_, AppState>) -> ConfigStatus {
    with_health(&state, config::status(&state.env_path))
}

fn with_health(state: &AppState, mut status: ConfigStatus) -> ConfigStatus {
    status.ps_hash_rejected_at = state
        .db
        .with(crate::commands::prices::ps_hash_rejected_at)
        .unwrap_or_else(|e| {
            log::warn!("could not read the PlayStation query state: {e}");
            None
        });
    status
}

/// Re-read `.env` after the user has edited it, so new keys take effect
/// without restarting the app.
#[tauri::command]
pub fn reload_config(state: State<'_, AppState>) -> Result<ConfigStatus> {
    // dotenvy never overwrites a variable that is already set, so clear the
    // ones we own first — otherwise an edited value would be silently ignored.
    for (name, _, _) in config::KEYS {
        std::env::remove_var(name);
    }

    if let Err(e) = dotenvy::from_path(&state.env_path) {
        log::warn!("reload failed for {}: {e}", state.env_path.display());
    }

    *state.credentials.write().unwrap_or_else(|e| e.into_inner()) = Credentials::from_env();
    // Without this the new keys sit in state but the clients still hold the old ones.
    state.rebuild_clients();

    Ok(with_health(&state, config::status(&state.env_path)))
}

/// Show the `.env` in Finder / File Explorer so the user can edit it.
#[tauri::command]
pub fn reveal_env_file(state: State<'_, AppState>) -> Result<()> {
    tauri_plugin_opener::reveal_item_in_dir(&state.env_path)
        .map_err(|e| AppError::Config(format!("could not open the credentials folder: {e}")))
}
