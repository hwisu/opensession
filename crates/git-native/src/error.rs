use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum GitStorageError {
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    #[error("git error: {0}")]
    Gix(Box<dyn std::error::Error + Send + Sync>),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, GitStorageError>;
