use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::clients::cheapshark::CheapShark;
use crate::clients::igdb::Igdb;
use crate::clients::itad::Itad;
use crate::clients::ps_store::PsStore;
use crate::config::Credentials;
use crate::db::Db;
use crate::error::{AppError, Result};

/// Shared application state, managed by Tauri and injected into commands.
pub struct AppState {
    pub db: Db,
    /// Behind a lock so `reload_config` can pick up edits to `.env` without
    /// restarting the app.
    pub credentials: RwLock<Credentials>,
    /// Rebuilt whenever credentials change. `None` until IGDB keys exist, so
    /// the app still launches and can explain what is missing.
    pub igdb: RwLock<Option<Arc<Igdb>>>,
    /// Region-aware PC prices. `None` until an ITAD key exists, at which point
    /// it supersedes CheapShark.
    pub itad: RwLock<Option<Arc<Itad>>>,
    /// Keyless PC prices, always available so the app works before setup.
    pub cheapshark: Arc<CheapShark>,
    /// Built lazily so a rotated query hash can be fixed by editing the file
    /// and reloading, rather than restarting the app.
    pub ps_store: RwLock<Option<Arc<PsStore>>>,
    /// Alerts raised by a refresh, drained by whoever can show them. Keeps the
    /// pricing code free of any dependency on the notification plugin.
    pub pending_alerts: Mutex<Vec<crate::commands::prices::PriceAlert>>,
    pub env_path: PathBuf,
}

impl AppState {
    pub fn credentials(&self) -> Credentials {
        self.credentials
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The IGDB client, or a message the user can act on.
    pub fn igdb(&self) -> Result<Arc<Igdb>> {
        self.igdb
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                AppError::Config(
                    "IGDB credentials are missing. Add IGDB_CLIENT_ID and \
                     IGDB_CLIENT_SECRET to your .env, then reload them in Settings."
                        .into(),
                )
            })
    }

    /// The PlayStation Store client. Needs no credentials, only the persisted
    /// query hashes, so it is built on first use.
    pub fn ps_store(&self) -> Result<Arc<PsStore>> {
        if let Some(existing) = self.ps_store.read().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(existing);
        }
        let built = Arc::new(PsStore::new(self.env_path.parent())?);
        *self.ps_store.write().unwrap_or_else(|e| e.into_inner()) = Some(built.clone());
        Ok(built)
    }

    /// The ITAD client, if a key is configured.
    pub fn itad(&self) -> Option<Arc<Itad>> {
        self.itad.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Rebuild every credentialed client. Called at startup and again after
    /// `.env` is reloaded, so new keys take effect without a restart.
    pub fn rebuild_clients(&self) {
        self.rebuild_igdb();
        self.rebuild_itad();
        // Dropped rather than rebuilt: the next request constructs it, picking
        // up any edit to ps-store-queries.json.
        *self.ps_store.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn rebuild_itad(&self) {
        let client = self.credentials().itad_api_key.and_then(|key| match Itad::new(key) {
            Ok(c) => Some(Arc::new(c)),
            Err(e) => {
                log::error!("could not build the IsThereAnyDeal client: {e}");
                None
            }
        });
        if client.is_none() {
            log::info!("no IsThereAnyDeal key — falling back to CheapShark (USD)");
        }
        *self.itad.write().unwrap_or_else(|e| e.into_inner()) = client;
    }

    /// Build the IGDB client from the current credentials. Called at startup
    /// and again after `.env` is reloaded.
    pub fn rebuild_igdb(&self) {
        let creds = self.credentials();
        let client = match (creds.igdb_client_id, creds.igdb_client_secret) {
            (Some(id), Some(secret)) => match Igdb::new(id, secret) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    log::error!("could not build the IGDB client: {e}");
                    None
                }
            },
            _ => None,
        };

        if client.is_none() {
            log::warn!("IGDB credentials absent — search and metadata are unavailable");
        }
        *self.igdb.write().unwrap_or_else(|e| e.into_inner()) = client;
    }
}
