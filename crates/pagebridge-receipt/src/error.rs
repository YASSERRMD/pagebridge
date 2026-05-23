use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiptError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("signature: {0}")]
    Signature(String),
    #[error("canonical encoding: {0}")]
    Canonical(String),
    #[error("verifier rejected receipt: {0}")]
    Rejected(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, ReceiptError>;
