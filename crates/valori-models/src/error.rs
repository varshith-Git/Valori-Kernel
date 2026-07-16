use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("verification failed: {0}")]
    Verify(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("install conflict: {0} is already being installed by another process")]
    InstallConflict(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ModelResult<T> = Result<T, ModelError>;
