use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeterministicError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("snapshot id mismatch: expected {expected}, computed {computed}")]
    SnapshotMismatch { expected: String, computed: String },
    #[error("provider {provider} does not honour determinism: {reason}")]
    NonDeterministicProvider { provider: String, reason: String },
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, DeterministicError>;
