use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_postgres::error::ErrorPosition;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    #[error("invalid connection URL: {0}")]
    InvalidConnectionUrl(String),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Cancelled(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PgErrorInfo {
    pub message: String,
    pub severity: Option<String>,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub code: Option<String>,
    pub position: Option<usize>,
}

impl CoreError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn pg_info(&self) -> Option<PgErrorInfo> {
        if let Self::Cancelled(message) = self {
            return Some(PgErrorInfo {
                message: message.clone(),
                severity: None,
                detail: None,
                hint: None,
                code: Some("57014".into()),
                position: None,
            });
        }
        let Self::Postgres(error) = self else {
            return None;
        };

        let db = error.as_db_error()?;
        let position = db.position().map(|position| match position {
            ErrorPosition::Original(position) => *position as usize,
            ErrorPosition::Internal { position, .. } => *position as usize,
        });

        Some(PgErrorInfo {
            message: db.message().to_string(),
            severity: Some(db.severity().to_string()),
            detail: db.detail().map(ToString::to_string),
            hint: db.hint().map(ToString::to_string),
            code: Some(db.code().code().to_string()),
            position,
        })
    }
}
