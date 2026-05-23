use thiserror::Error;

#[derive(Debug, Error)]
pub enum TimeTravelError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("no snapshot prior to requested timestamp")]
    NoSnapshotBefore,
    #[error("snapshot store: {0}")]
    Store(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, TimeTravelError>;
