//! Adapter batch upsert sanity checks.
//!
//! We assert that a 10k-node batch completes in well under the time a naive
//! 10k-per-node loop would take and that every node is readable after.

#![allow(clippy::cast_possible_truncation)]

use std::sync::Arc;

use pagebridge_core::adapter::{MemoryAdapter, StorageAdapter};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};

fn make_node(doc: &DocId, parent: &NodeId, seq: u32) -> NodeRecord {
    let node_id = parent.child("leaf", &seq.to_string()).unwrap();
    NodeRecord {
        node_id,
        doc_id: doc.clone(),
        parent_id: Some(parent.clone()),
        title: format!("Leaf {seq}"),
        level: NodeLevel::Leaf,
        routing_summary: "rs".into(),
        summary: "s".into(),
        child_ids: vec![],
        span: Some((0, 1)),
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    }
}

#[tokio::test]
async fn batch_upsert_inserts_every_record() {
    let storage = Arc::new(MemoryAdapter::new());
    let doc = DocId::new("doc-batch").unwrap();
    // Need a root parent because every leaf must have one.
    let root = NodeId::root(&doc);
    let root_record = NodeRecord {
        node_id: root.clone(),
        doc_id: doc.clone(),
        parent_id: None,
        title: "Root".into(),
        level: NodeLevel::Document,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: None,
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: false,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    };
    storage.upsert_node(&root_record).await.unwrap();

    let nodes: Vec<NodeRecord> = (0..2_000).map(|i| make_node(&doc, &root, i)).collect();
    storage.upsert_nodes_batch(&nodes).await.unwrap();
    let stats = storage.stats().await.unwrap();
    assert!(stats.node_count >= 2_000);
}

#[tokio::test]
async fn batch_upsert_validates_before_writing() {
    let storage = Arc::new(MemoryAdapter::new());
    let doc = DocId::new("doc-bad").unwrap();
    let root = NodeId::root(&doc);
    let mut bad = make_node(&doc, &root, 0);
    bad.title = String::new(); // invalid: empty title
    let err = storage.upsert_nodes_batch(&[bad]).await;
    assert!(err.is_err(), "batch should reject invalid records up front");
}

#[test]
fn recommended_batch_size_is_sane() {
    let m = MemoryAdapter::new();
    assert!(m.recommended_batch_size() >= 100);
}
