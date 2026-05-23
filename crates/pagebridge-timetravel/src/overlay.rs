//! Backward-replay overlay.
//!
//! Given a base [`CorpusSnapshot`] and an ordered list of mutation
//! events that occurred *between* the base and the requested timestamp,
//! reconstruct the snapshot that was live at the timestamp.
//!
//! Mutation events are described as [`MutationEvent`]s; the audit log
//! adapter is responsible for translating its raw events into this
//! shape. The overlay does not depend on `pagebridge-audit` at the
//! schema level so it stays small and testable.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use pagebridge_core::id::NodeId;
use pagebridge_deterministic::{CorpusSnapshot, SnapshotEntry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MutationEvent {
    Insert(SnapshotEntry),
    Update(SnapshotEntry),
    Delete { node_id: NodeId },
}

#[derive(Debug, Clone, Default)]
pub struct Overlay {
    by_id: HashMap<NodeId, SnapshotEntry>,
}

impl Overlay {
    #[must_use]
    pub fn from_snapshot(snapshot: &CorpusSnapshot) -> Self {
        let mut by_id = HashMap::new();
        for e in &snapshot.entries {
            by_id.insert(e.node_id.clone(), e.clone());
        }
        Self { by_id }
    }

    /// Apply every event in chronological order to advance the overlay
    /// to the state that was live *immediately after* the last event.
    pub fn apply_forward<I: IntoIterator<Item = MutationEvent>>(&mut self, events: I) {
        for e in events {
            match e {
                MutationEvent::Insert(entry) | MutationEvent::Update(entry) => {
                    self.by_id.insert(entry.node_id.clone(), entry);
                }
                MutationEvent::Delete { node_id } => {
                    self.by_id.remove(&node_id);
                }
            }
        }
    }

    #[must_use]
    pub fn as_snapshot(
        &self,
        workspace: pagebridge_core::workspace::WorkspaceId,
    ) -> CorpusSnapshot {
        let mut entries: Vec<SnapshotEntry> = self.by_id.values().cloned().collect();
        entries.sort_by(|a, b| a.node_id.as_str().cmp(b.node_id.as_str()));
        CorpusSnapshot::new(workspace, entries)
    }

    #[must_use]
    pub fn contains(&self, id: &NodeId) -> bool {
        self.by_id.contains_key(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::id::DocId;
    use pagebridge_core::workspace::WorkspaceId;

    fn entry(name: &str, hash: &str, v: u32) -> SnapshotEntry {
        let doc = DocId::new("doc").unwrap();
        SnapshotEntry {
            node_id: NodeId::root(&doc).child("sec", name).unwrap(),
            content_hash_hex: hash.into(),
            version: v,
        }
    }

    #[test]
    fn forward_replay_applies_insert_update_delete() {
        let ws = WorkspaceId::new("acme").unwrap();
        let base = CorpusSnapshot::new(ws.clone(), vec![entry("a", "11", 1)]);
        let mut overlay = Overlay::from_snapshot(&base);
        overlay.apply_forward([
            MutationEvent::Insert(entry("b", "22", 1)),
            MutationEvent::Update(entry("a", "33", 2)),
            MutationEvent::Delete {
                node_id: entry("b", "22", 1).node_id,
            },
        ]);
        let snap = overlay.as_snapshot(ws);
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].content_hash_hex, "33");
        assert_eq!(snap.entries[0].version, 2);
    }
}
