//! Error type for the entire pagebridge crate stack.

use crate::id::{DocId, NodeId};

/// All errors produced by pagebridge funnel through this enum.
#[derive(thiserror::Error, Debug)]
pub enum PagebridgeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(String),

    #[error("adapter error ({adapter}): {message}")]
    Adapter { adapter: String, message: String },

    #[error("llm error ({provider}): {message}")]
    Llm { provider: String, message: String },

    #[error("invalid node id: {0}")]
    InvalidNodeId(String),

    #[error("invalid doc id: {0}")]
    InvalidDocId(String),

    #[error("node not found: {0:?}")]
    NodeNotFound(NodeId),

    #[error("document not found: {0:?}")]
    DocumentNotFound(DocId),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("token budget exceeded: requested {requested}, available {available}")]
    TokenBudget { requested: u32, available: u32 },

    #[error("navigation halted: {reason}")]
    NavigationHalted { reason: String },

    #[error("parse error ({source_kind}): {message}")]
    Parse {
        source_kind: String,
        message: String,
    },

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for PagebridgeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, PagebridgeError>;
