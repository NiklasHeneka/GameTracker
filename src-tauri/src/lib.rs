mod clients;
mod commands;
mod config;
mod db;
mod error;
mod services;
mod settings;
mod state;
mod util;

use std::sync::RwLock;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let handle = app.handle().clone();

            // Credentials first: the .env template is created on first run so
            // the Settings screen can point at a file that actually exists.
            let (credentials, env_path) = config::load(&handle)?;

            let db_path = handle.path().app_data_dir()?.join("gametracker.db");
            let db = db::Db::open(&db_path)?;

            // Materialise defaults on first run so the settings row is never
            // absent for later readers.
            db.with(|conn| {
                let current = settings::read(conn)?;
                settings::write(conn, &current)
            })?;

            app.manage(state::AppState {
                db,
                credentials: RwLock::new(credentials),
                igdb: RwLock::new(None),
                itad: RwLock::new(None),
                cheapshark: std::sync::Arc::new(clients::cheapshark::CheapShark::new()?),
                ps_store: RwLock::new(None),
                pending_alerts: std::sync::Mutex::new(Vec::new()),
                env_path,
            });
            app.state::<state::AppState>().rebuild_clients();

            services::refresh::spawn(handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::config::config_status,
            commands::config::reload_config,
            commands::config::reveal_env_file,
            commands::search::search_games,
            commands::search::get_game,
            commands::library::list_entries,
            commands::library::add_entry,
            commands::library::update_entry,
            commands::library::reorder_entries,
            commands::library::delete_entry,
            commands::prices::get_prices,
            commands::prices::refresh_wishlist_prices,
            commands::prices::list_active_deals,
            commands::prices::set_manual_price,
            commands::queue::list_queue,
            commands::queue::enqueue,
            commands::queue::dequeue,
            commands::queue::reorder_queue,
            commands::import::preview_steam_import,
            commands::import::import_steam_library,
            commands::stats::get_stats,
            commands::backup::export_backup,
            commands::backup::import_backup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
