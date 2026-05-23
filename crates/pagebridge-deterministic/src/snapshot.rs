//! Corpus snapshots.
//!
//! A snapshot freezes every node in the corpus to its current content
//! hash and produces a single content-addressed identifier (the sha256
//! over sorted (node_id, content_hash) tuples). Two corpora with the
//! same snapshot id are byte-identical at the node level.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use pagebridge_core::id::NodeId;
use pagebridge_core::workspace::WorkspaceId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub node_id: NodeId,
    pub content_hash_hex: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSnapshot {
    pub snapshot_id: String,
    pub workspace_id: WorkspaceId,
    pub created_at_ns: u128,
    pub node_count: u32,
    pub entries: Vec<SnapshotEntry>,
}

#[must_use]
pub fn compute_snapshot_id(entries: &[SnapshotEntry]) -> String {
    let mut sorted: Vec<&SnapshotEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.node_id.as_str().cmp(b.node_id.as_str()));
    let mut h = Sha256::new();
    for e in sorted {
        h.update(e.node_id.as_str().as_bytes());
        h.update(b"|");
        h.update(e.content_hash_hex.as_bytes());
        h.update(b"|");
        h.update(e.version.to_be_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

impl CorpusSnapshot {
    #[must_use]
    pub fn new(workspace_id: WorkspaceId, mut entries: Vec<SnapshotEntry>) -> Self {
        entries.sort_by(|a, b| a.node_id.as_str().cmp(b.node_id.as_str()));
        let snapshot_id = compute_snapshot_id(&entries);
        let node_count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        Self {
            snapshot_id,
            workspace_id,
            created_at_ns: now_ns(),
            node_count,
            entries,
        }
    }

    pub fn matches(&self, expected_id: &str) -> bool {
        self.snapshot_id == expected_id
    }
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::id::DocId;

    fn entry(slug: &str, hash: &str) -> SnapshotEntry {
        let doc = DocId::new("doc").unwrap();
        SnapshotEntry {
            node_id: NodeId::root(&doc).child("sec", slug).unwrap(),
            content_hash_hex: hash.into(),
            version: 1,
        }
    }

    #[test]
    fn snapshot_id_is_stable_under_reorder() {
        let a = vec![entry("a", "11"), entry("b", "22")];
        let b = vec![entry("b", "22"), entry("a", "11")];
        assert_eq!(compute_snapshot_id(&a), compute_snapshot_id(&b));
    }

    #[test]
    fn changing_content_changes_id() {
        let a = vec![entry("a", "11")];
        let b = vec![entry("a", "12")];
        assert_ne!(compute_snapshot_id(&a), compute_snapshot_id(&b));
    }
}
