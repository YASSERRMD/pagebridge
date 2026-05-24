//! Verifies the lazy tantivy commit scheduler.

use std::sync::Arc;
use std::time::Duration;

use pagebridge_adapter_embedded::{CommitSchedulerConfig, EmbeddedAdapter};
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use tempfile::tempdir;

fn make_root(doc: &DocId) -> NodeRecord {
    NodeRecord {
        node_id: NodeId::root(doc),
        doc_id: doc.clone(),
        parent_id: None,
        title: format!("Document {doc}"),
        level: NodeLevel::Document,
        routing_summary: "root".into(),
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
    }
}

fn make_leaf(doc: &DocId, root: &NodeId, seq: u32) -> NodeRecord {
    let leaf_id = root.child("leaf", &seq.to_string()).unwrap();
    NodeRecord {
        node_id: leaf_id,
        doc_id: doc.clone(),
        parent_id: Some(root.clone()),
        title: format!("Leaf {seq}"),
        level: NodeLevel::Leaf,
        routing_summary: format!("toc {seq}"),
        summary: format!("body of leaf {seq} with keyword pagebridgesearchneedle"),
        child_ids: vec![],
        span: Some((0, 1)),
        page_start: None,
        page_end: None,
        keywords: vec!["pagebridgesearchneedle".to_owned()],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    }
}

#[tokio::test]
async fn search_invisible_until_flush_with_high_thresholds() {
    let dir = tempdir().unwrap();
    let cfg = CommitSchedulerConfig {
        max_dirty_docs: 10_000,
        max_dirty_age: Duration::from_secs(3600),
    };
    let store = Arc::new(EmbeddedAdapter::open_with_commit_config(dir.path(), cfg).unwrap());
    store.migrate().await.unwrap();
    let doc = DocId::new("doc-flush").unwrap();
    let root = NodeId::root(&doc);
    store.upsert_node(&make_root(&doc)).await.unwrap();
    for i in 0..50 {
        store.upsert_node(&make_leaf(&doc, &root, i)).await.unwrap();
    }
    // No commit yet (threshold = 10k docs, age = 1h) so BM25 sees nothing.
    let hits = store.bm25_search("pagebridgesearchneedle", 10).await.unwrap();
    assert!(
        hits.is_empty(),
        "expected zero hits before flush, got {}",
        hits.len()
    );
    // Force a flush; now search must see the leaves.
    store.flush().await.unwrap();
    let hits = store.bm25_search("pagebridgesearchneedle", 10).await.unwrap();
    assert!(
        !hits.is_empty(),
        "expected hits after flush, got {}",
        hits.len()
    );
}

#[tokio::test]
async fn dirty_count_threshold_triggers_commit() {
    let dir = tempdir().unwrap();
    let cfg = CommitSchedulerConfig {
        max_dirty_docs: 5,
        max_dirty_age: Duration::from_secs(3600),
    };
    let store = Arc::new(EmbeddedAdapter::open_with_commit_config(dir.path(), cfg).unwrap());
    store.migrate().await.unwrap();
    let doc = DocId::new("doc-thresh").unwrap();
    let root = NodeId::root(&doc);
    store.upsert_node(&make_root(&doc)).await.unwrap();
    for i in 0..6 {
        store.upsert_node(&make_leaf(&doc, &root, i)).await.unwrap();
    }
    // After the 5th leaf the scheduler commits automatically.
    let hits = store.bm25_search("pagebridgesearchneedle", 10).await.unwrap();
    assert!(!hits.is_empty(), "expected auto-commit at threshold");
}
