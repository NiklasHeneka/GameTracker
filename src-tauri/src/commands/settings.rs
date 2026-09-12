use tauri::State;

use crate::error::Result;
use crate::settings::{self, Settings, SettingsPatch};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    state.db.with(settings::read)
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> Result<Settings> {
    state.db.with(|conn| settings::patch(conn, patch))
}
