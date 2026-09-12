use serde::Serialize;

/// Every failure that can cross the IPC boundary. Serialised as
/// `{ kind, message }` so the frontend can branch on `kind` without
/// string-matching human-readable text.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Tauri(#[from] tauri::Error),

    #[error("invalid data: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    /// An upstream API answered with a non-success status.
    #[error("{service} returned {status}: {body}")]
    Api {
        service: &'static str,
        status: u16,
        body: String,
    },

    /// A credential is missing or the .env could not be resolved.
    #[error("{0}")]
    Config(String),

    #[allow(dead_code)] // returned by the library commands from Phase 1 onwards
    #[error("not found: {0}")]
    NotFound(String),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::Db(_) => "db",
            AppError::Io(_) => "io",
            AppError::Tauri(_) => "tauri",
            AppError::Serde(_) => "serde",
            AppError::Http(_) => "http",
            AppError::Api { .. } => "api",
            AppError::Config(_) => "config",
            AppError::NotFound(_) => "not_found",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
