//! Errors emitted by the audit log subsystem.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("signature: {0}")]
    Signature(String),

    #[error("chain broken at event_id {at}: {detail}")]
    ChainBroken { at: String, detail: String },

    #[error("merkle proof failed at batch {batch}: {detail}")]
    MerkleProof { batch: u64, detail: String },

    #[error("sink ({sink}): {message}")]
    Sink { sink: String, message: String },

    #[error("config: {0}")]
    Config(String),

    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AuditError>;
