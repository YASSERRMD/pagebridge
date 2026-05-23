//! Open Retrieval Response Format (ORRF) v1.
//!
//! Vendor-neutral JSON schema for retrieval responses. The spec lives
//! at `docs/spec/ORRF-v1.md`; this crate is the reference Rust
//! implementation. Any RAG product can adopt ORRF and pass the bundled
//! conformance suite.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrrfError {
    #[error("missing field: {0}")]
    Missing(String),
    #[error("invalid field {0}: {1}")]
    Invalid(String, String),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OrrfError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrrfCitation {
    pub id: String,
    pub content_hash_hex: String,
    pub source_uri: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrrfTraceSummary {
    pub steps: u32,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrrfReceipt {
    pub answer_hash_hex: String,
    pub signature_hex: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrrfResponse {
    pub orrf_version: u32,
    pub question: String,
    pub answer: String,
    pub citations: Vec<OrrfCitation>,
    pub trace: OrrfTraceSummary,
    pub receipt: Option<OrrfReceipt>,
    pub confidence: Option<f32>,
}

impl OrrfResponse {
    /// Validate a response against the v1 schema. Returns the first
    /// failure or Ok(()).
    pub fn validate(&self) -> Result<()> {
        if self.orrf_version != 1 {
            return Err(OrrfError::Invalid(
                "orrf_version".into(),
                format!("expected 1, got {}", self.orrf_version),
            ));
        }
        for (i, c) in self.citations.iter().enumerate() {
            if c.content_hash_hex.len() != 64 {
                return Err(OrrfError::Invalid(
                    format!("citations[{i}].content_hash_hex"),
                    "expected 64 hex chars".into(),
                ));
            }
        }
        if let Some(r) = &self.receipt {
            if r.answer_hash_hex.len() != 64 {
                return Err(OrrfError::Invalid(
                    "receipt.answer_hash_hex".into(),
                    "expected 64 hex chars".into(),
                ));
            }
            if r.signature_hex.len() != 128 {
                return Err(OrrfError::Invalid(
                    "receipt.signature_hex".into(),
                    "expected 128 hex chars (64-byte Ed25519 signature)".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OrrfResponse {
        OrrfResponse {
            orrf_version: 1,
            question: "q".into(),
            answer: "a".into(),
            citations: vec![OrrfCitation {
                id: "c1".into(),
                content_hash_hex: "a".repeat(64),
                source_uri: "doc://policy/sec/1".into(),
                version: 1,
            }],
            trace: OrrfTraceSummary {
                steps: 3,
                total_input_tokens: 100,
                total_output_tokens: 50,
                duration_ms: 800,
            },
            receipt: None,
            confidence: Some(0.9),
        }
    }

    #[test]
    fn valid_response_passes() {
        sample().validate().unwrap();
    }

    #[test]
    fn wrong_version_rejected() {
        let mut s = sample();
        s.orrf_version = 2;
        assert!(s.validate().is_err());
    }

    #[test]
    fn malformed_content_hash_rejected() {
        let mut s = sample();
        s.citations[0].content_hash_hex = "short".into();
        assert!(s.validate().is_err());
    }
}
