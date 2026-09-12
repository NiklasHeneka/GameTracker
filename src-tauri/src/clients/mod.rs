pub mod cheapshark;
pub mod igdb;
pub mod itad;
pub mod ps_store;
pub mod steam;

/// Identify this client to every API we call. CheapShark rejects requests with
/// a missing or generic User-Agent outright, and it is simply good manners on
/// free endpoints.
pub fn user_agent() -> String {
    format!("GameTracker/{} (personal, non-commercial)", env!("CARGO_PKG_VERSION"))
}
