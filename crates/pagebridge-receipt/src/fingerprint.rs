//! Components of the answer receipt that describe how the answer was
//! produced: the LLM fingerprint, the referenced nodes, the prompt
//! versions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use pagebridge_core::id::NodeId;

/// One node that contributed to the synthesized answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeReference {
    pub node_id: NodeId,
    /// sha256(content_bytes) of the leaf at retrieval time.
    pub content_hash_hex: String,
    /// Monotonically increasing per-node version. The embedded/SQL adapters
    /// increment this on every update; older versions are kept in the
    /// audit log only.
    pub version: u32,
}

/// Sampling parameters and identity of the LLM call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmFingerprint {
    pub provider: String,
    pub model: String,
    pub temperature_milli: u32, // store as 1000x to keep canonical encoding integer-only
    pub top_p_milli: u32,
    pub seed: u64,
    /// Optional vendor-specific revision string (e.g., a model SHA).
    pub revision: Option<String>,
}

impl LlmFingerprint {
    /// Serialize to bytes in a way that is stable across pagebridge versions:
    /// sorted-key JSON with integer-only numerics.
    #[must_use]
    pub fn canonical(&self) -> Vec<u8> {
        serde_json::to_vec(&CanonLlm {
            provider: &self.provider,
            model: &self.model,
            revision: self.revision.as_deref().unwrap_or(""),
            seed: self.seed,
            temperature_milli: self.temperature_milli,
            top_p_milli: self.top_p_milli,
        })
        .expect("serializable")
    }
}

#[derive(Serialize)]
struct CanonLlm<'a> {
    model: &'a str,
    provider: &'a str,
    revision: &'a str,
    seed: u64,
    temperature_milli: u32,
    top_p_milli: u32,
}

/// Prompt template name -> version pairs that were active at the time
/// the answer was produced.
pub type PromptVersionMap = BTreeMap<String, u32>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_encoding_is_stable_under_field_reorder() {
        let a = LlmFingerprint {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            temperature_milli: 0,
            top_p_milli: 1000,
            seed: 42,
            revision: Some("0125".into()),
        };
        let b = LlmFingerprint {
            seed: 42,
            top_p_milli: 1000,
            temperature_milli: 0,
            revision: Some("0125".into()),
            model: "gpt-4o-mini".into(),
            provider: "openai".into(),
        };
        assert_eq!(a.canonical(), b.canonical());
    }
}
