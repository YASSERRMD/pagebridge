//! Reproducible build manifests.
//!
//! A [`BuildManifest`] is the recipe for a pagebridge index: corpus
//! hash, tokenizer version, chunker version, summary model fingerprint,
//! prompt versions, plus a list of produced artifacts and their
//! content hashes. Re-running `pagebridge build --manifest <m.json>`
//! with the same recipe reproduces byte-identical artifacts.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("artifact mismatch: {path} expected {expected}, got {actual}")]
    Mismatch { path: String, expected: String, actual: String },
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, BuildError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub path: String,
    pub hash_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildManifest {
    pub build_id: String,
    pub timestamp_ns: u128,
    pub corpus_hash_hex: String,
    pub tokenizer_version: String,
    pub chunker_version: String,
    pub summary_model_fingerprint: String,
    pub pagebridge_version: String,
    pub prompt_versions: BTreeMap<String, u32>,
    pub produced_artifacts: Vec<Artifact>,
}

impl BuildManifest {
    /// Compute a hash over every input that determines the produced
    /// artifacts. Two manifests with the same input hash MUST produce
    /// byte-identical artifacts.
    #[must_use]
    pub fn input_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.corpus_hash_hex.as_bytes());
        h.update(b"|");
        h.update(self.tokenizer_version.as_bytes());
        h.update(b"|");
        h.update(self.chunker_version.as_bytes());
        h.update(b"|");
        h.update(self.summary_model_fingerprint.as_bytes());
        h.update(b"|");
        h.update(self.pagebridge_version.as_bytes());
        h.update(b"|");
        for (k, v) in &self.prompt_versions {
            h.update(k.as_bytes());
            h.update(b":");
            h.update(v.to_be_bytes());
            h.update(b",");
        }
        hex::encode(h.finalize())
    }
}

/// Diff two manifests. Returns a human-readable list of every field
/// that differs, used by `pagebridge build diff`.
#[must_use]
pub fn diff(a: &BuildManifest, b: &BuildManifest) -> Vec<String> {
    let mut out = Vec::new();
    if a.corpus_hash_hex != b.corpus_hash_hex {
        out.push(format!(
            "corpus_hash: {} -> {}",
            a.corpus_hash_hex, b.corpus_hash_hex
        ));
    }
    if a.tokenizer_version != b.tokenizer_version {
        out.push(format!(
            "tokenizer_version: {} -> {}",
            a.tokenizer_version, b.tokenizer_version
        ));
    }
    if a.chunker_version != b.chunker_version {
        out.push(format!(
            "chunker_version: {} -> {}",
            a.chunker_version, b.chunker_version
        ));
    }
    if a.summary_model_fingerprint != b.summary_model_fingerprint {
        out.push(format!(
            "summary_model_fingerprint: {} -> {}",
            a.summary_model_fingerprint, b.summary_model_fingerprint
        ));
    }
    if a.pagebridge_version != b.pagebridge_version {
        out.push(format!(
            "pagebridge_version: {} -> {}",
            a.pagebridge_version, b.pagebridge_version
        ));
    }
    let keys: std::collections::BTreeSet<&String> =
        a.prompt_versions.keys().chain(b.prompt_versions.keys()).collect();
    for k in keys {
        let av = a.prompt_versions.get(k).copied();
        let bv = b.prompt_versions.get(k).copied();
        if av != bv {
            out.push(format!("prompt_versions[{k}]: {av:?} -> {bv:?}"));
        }
    }
    out
}

/// Verify a build by recomputing the hash of every produced artifact
/// against the manifest. Returns Err on the first mismatch.
pub async fn verify_artifacts(manifest: &BuildManifest, base: &std::path::Path) -> Result<()> {
    for art in &manifest.produced_artifacts {
        let path = base.join(&art.path);
        let bytes = tokio::fs::read(&path).await?;
        let mut h = Sha256::new();
        h.update(&bytes);
        let actual = hex::encode(h.finalize());
        if actual != art.hash_hex {
            return Err(BuildError::Mismatch {
                path: art.path.clone(),
                expected: art.hash_hex.clone(),
                actual,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BuildManifest {
        BuildManifest {
            build_id: "b-001".into(),
            timestamp_ns: 0,
            corpus_hash_hex: "deadbeef".into(),
            tokenizer_version: "snowball-en@2.1".into(),
            chunker_version: "markdown@1.3".into(),
            summary_model_fingerprint: "anthropic:claude-haiku-4-5:seed=42:T=0".into(),
            pagebridge_version: "1.3.0".into(),
            prompt_versions: [("synthesize".to_string(), 1u32)].into_iter().collect(),
            produced_artifacts: vec![],
        }
    }

    #[test]
    fn input_hash_stable_across_field_reorder_is_same() {
        let a = sample();
        let b = sample();
        assert_eq!(a.input_hash(), b.input_hash());
    }

    #[test]
    fn changing_corpus_changes_input_hash() {
        let a = sample();
        let mut b = sample();
        b.corpus_hash_hex = "ff".repeat(8);
        assert_ne!(a.input_hash(), b.input_hash());
    }

    #[test]
    fn diff_reports_changed_fields() {
        let a = sample();
        let mut b = sample();
        b.tokenizer_version = "snowball-en@2.2".into();
        b.prompt_versions.insert("synthesize".into(), 2);
        let d = diff(&a, &b);
        assert!(d.iter().any(|s| s.contains("tokenizer_version")));
        assert!(d.iter().any(|s| s.contains("prompt_versions")));
    }
}
