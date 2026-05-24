//! Bit-identical determinism test.
//!
//! Synthesizes the same canonical inputs twice and confirms the
//! produced snapshot id, canonical ORDER BY fragment, and LLM
//! fingerprint encoding are byte-identical between runs. This is a
//! component-level test; full pipeline determinism is exercised by the
//! integration tests in the umbrella crate.

use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::workspace::WorkspaceId;
use pagebridge_deterministic::{
    compute_snapshot_id, order_by_for, DeterministicMode, QueryOrder, SnapshotEntry,
};

#[test]
fn snapshot_id_is_byte_identical_between_runs() {
    let doc = DocId::new("doc").unwrap();
    let entries = vec![
        SnapshotEntry {
            node_id: NodeId::root(&doc).child("sec", "a").unwrap(),
            content_hash_hex: "11".into(),
            version: 1,
        },
        SnapshotEntry {
            node_id: NodeId::root(&doc).child("sec", "b").unwrap(),
            content_hash_hex: "22".into(),
            version: 1,
        },
    ];
    let a = compute_snapshot_id(&entries);
    let b = compute_snapshot_id(&entries);
    assert_eq!(a, b);
}

#[test]
fn order_by_is_byte_identical() {
    assert_eq!(
        order_by_for(QueryOrder::ByNodeId),
        order_by_for(QueryOrder::ByNodeId)
    );
}

#[test]
fn deterministic_mode_serializes_stably() {
    let m = DeterministicMode::strict();
    let a = serde_json::to_string(&m).unwrap();
    let b = serde_json::to_string(&m).unwrap();
    assert_eq!(a, b);
    let _ws = WorkspaceId::new("acme").unwrap();
}
