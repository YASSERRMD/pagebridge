//! Transparency-log anchoring (Sigstore Rekor / Trillian).
//!
//! When configured, every Merkle batch root is submitted to a public
//! append-only log so external observers can later prove the batch
//! existed at the timestamp it was published. The Rust client lives in
//! the `sigstore` / `trillian-rs` crates which we wire optionally.
//!
//! In the default build (no `http-sinks` feature), this module exposes
//! a NoopTransparencyClient that records calls in memory; production
//! deployments swap in a real client.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::merkle::MerkleBatch;

/// A transparency-log entry recorded for one Merkle batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrillianEntry {
    pub log_id: String,
    pub leaf_index: u64,
    pub leaf_hash_hex: String,
    pub anchored_at_ms: i64,
    pub proof: Vec<String>,
}

#[async_trait::async_trait]
pub trait TransparencyClient: Send + Sync + 'static {
    async fn anchor(&self, batch: &MerkleBatch) -> Result<TrillianEntry>;
}

/// In-memory client used in tests and in builds without `http-sinks`.
pub struct NoopTransparencyClient {
    log_id: String,
    inner: Arc<Mutex<Vec<MerkleBatch>>>,
}

impl NoopTransparencyClient {
    #[must_use]
    pub fn new(log_id: impl Into<String>) -> Self {
        Self {
            log_id: log_id.into(),
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn recorded(&self) -> Vec<MerkleBatch> {
        self.inner.lock().clone()
    }
}

#[async_trait::async_trait]
impl TransparencyClient for NoopTransparencyClient {
    async fn anchor(&self, batch: &MerkleBatch) -> Result<TrillianEntry> {
        let mut g = self.inner.lock();
        let idx = g.len() as u64;
        g.push(batch.clone());
        Ok(TrillianEntry {
            log_id: self.log_id.clone(),
            leaf_index: idx,
            leaf_hash_hex: hex::encode(batch.root),
            anchored_at_ms: 0,
            proof: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_records_anchors() {
        let c = NoopTransparencyClient::new("local");
        let batch = MerkleBatch {
            batch_id: 0,
            workspace_id: "acme".into(),
            first_event_id: "a".into(),
            last_event_id: "b".into(),
            leaf_count: 1,
            root: [1u8; 32],
        };
        let entry = c.anchor(&batch).await.unwrap();
        assert_eq!(entry.leaf_index, 0);
        assert_eq!(c.recorded().len(), 1);
    }
}
