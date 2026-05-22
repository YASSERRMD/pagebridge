//! Integration tests for the embedded (redb + tantivy) adapter.

#![allow(clippy::redundant_clone)]

use pagebridge_adapter_embedded::EmbeddedAdapter;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::{DocumentEntry, SummaryCacheEntry};
use std::sync::Arc;
use tempfile::tempdir;

fn make_root(doc: &DocId) -> NodeRecord {
    NodeRecord {
        node_id: NodeId::root(doc),
        doc_id: doc.clone(),
        parent_id: None,
        title: format!("Document {doc}"),
        level: NodeLevel::Document,
        routing_summary: "the doc".into(),
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

fn make_leaf(doc: &DocId, sec: u32, leaf: u32, title: &str, kw: &[&str]) -> NodeRecord {
    let root = NodeId::root(doc);
    let sec_id = root.child("sec", &sec.to_string()).unwrap();
    let leaf_id = sec_id.child("leaf", &leaf.to_string()).unwrap();
    NodeRecord {
        node_id: leaf_id,
        doc_id: doc.clone(),
        parent_id: Some(sec_id),
        title: title.into(),
        level: NodeLevel::Leaf,
        routing_summary: format!("toc for {title}"),
        summary: format!("body of {title}"),
        child_ids: vec![],
        span: Some((0, 32)),
        page_start: Some(1),
        page_end: Some(1),
        keywords: kw.iter().map(|s| (*s).to_owned()).collect(),
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    }
}

fn make_section(doc: &DocId, sec: u32, title: &str) -> NodeRecord {
    let root = NodeId::root(doc);
    let sec_id = root.child("sec", &sec.to_string()).unwrap();
    NodeRecord {
        node_id: sec_id,
        doc_id: doc.clone(),
        parent_id: Some(root),
        title: title.into(),
        level: NodeLevel::Section,
        routing_summary: format!("toc for {title}"),
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

#[tokio::test]
async fn roundtrip_and_persistence() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();

    {
        let adapter = EmbeddedAdapter::open(&path).unwrap();
        adapter.migrate().await.unwrap();
        let doc = DocId::new("d1").unwrap();
        adapter.upsert_node(&make_root(&doc)).await.unwrap();
        adapter
            .upsert_node(&make_section(&doc, 1, "Intro"))
            .await
            .unwrap();
        adapter
            .upsert_node(&make_leaf(&doc, 1, 1, "Timeline", &["timeline", "rollout"]))
            .await
            .unwrap();
        adapter
            .upsert_node(&make_leaf(&doc, 1, 2, "Budget", &["budget", "cost"]))
            .await
            .unwrap();
        adapter
            .upsert_document(&DocumentEntry {
                doc_id: doc.clone(),
                title: "Doc 1".into(),
                source_kind: "markdown".into(),
                ingested_at: 1,
                root_node_id: NodeId::root(&doc),
                leaf_count: 2,
                byte_count: 0,
            })
            .await
            .unwrap();

        // BM25 should find the timeline leaf for the word "timeline".
        let hits = adapter.bm25_search("timeline", 10).await.unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.title.contains("Timeline")));
    }

    // Reopen the same directory.
    let adapter = EmbeddedAdapter::open(&path).unwrap();
    adapter.migrate().await.unwrap();
    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    let doc = DocId::new("d1").unwrap();
    let leaves = adapter.leaves_under(&NodeId::root(&doc)).await.unwrap();
    assert_eq!(leaves.len(), 2);
}

#[tokio::test]
async fn raw_text_and_summary_cache() {
    let dir = tempdir().unwrap();
    let adapter = EmbeddedAdapter::open(dir.path()).unwrap();
    adapter.migrate().await.unwrap();
    let doc = DocId::new("d2").unwrap();

    let off1 = adapter.put_raw(&doc, b"hello ").await.unwrap();
    let off2 = adapter.put_raw(&doc, b"world").await.unwrap();
    assert_eq!(off1, 0);
    assert_eq!(off2, 6);

    let text = adapter.read_raw_text(&doc, (0, 11)).await.unwrap();
    assert_eq!(text, "hello world");

    let span = adapter.read_raw_text(&doc, (6, 11)).await.unwrap();
    assert_eq!(span, "world");

    let hash = [9u8; 32];
    assert!(adapter.get_summary_cache(&hash).await.unwrap().is_none());
    let entry = SummaryCacheEntry {
        routing_summary: "rs".into(),
        summary: "s".into(),
        keywords: vec!["k".into()],
        model_fingerprint: "m".into(),
        created_at: 1,
    };
    adapter.upsert_summary_cache(&hash, &entry).await.unwrap();
    let back = adapter.get_summary_cache(&hash).await.unwrap().unwrap();
    assert_eq!(back.summary, "s");
}

#[tokio::test]
async fn concurrent_reads() {
    let dir = tempdir().unwrap();
    let adapter: Arc<dyn StorageAdapter> = Arc::new(EmbeddedAdapter::open(dir.path()).unwrap());
    adapter.migrate().await.unwrap();
    let doc = DocId::new("d3").unwrap();
    adapter.upsert_node(&make_root(&doc)).await.unwrap();
    adapter
        .upsert_node(&make_section(&doc, 1, "S1"))
        .await
        .unwrap();
    for i in 0..10 {
        adapter
            .upsert_node(&make_leaf(
                &doc,
                1,
                i,
                &format!("Leaf {i}"),
                &["alpha", "beta"],
            ))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();
    for _ in 0..16 {
        let a = Arc::clone(&adapter);
        handles.push(tokio::spawn(async move {
            let hits = a.bm25_search("alpha", 5).await.unwrap();
            assert!(!hits.is_empty());
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn children_and_path_walk() {
    let dir = tempdir().unwrap();
    let adapter = EmbeddedAdapter::open(dir.path()).unwrap();
    adapter.migrate().await.unwrap();

    let doc = DocId::new("d4").unwrap();
    adapter.upsert_node(&make_root(&doc)).await.unwrap();
    adapter
        .upsert_node(&make_section(&doc, 1, "S1"))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 1, "L1", &["a"]))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 2, "L2", &["b"]))
        .await
        .unwrap();

    let kids = adapter
        .children_summaries(&NodeId::root(&doc))
        .await
        .unwrap();
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].title, "S1");

    let leaf_kids = adapter
        .children_records(&NodeId::root(&doc).child("sec", "1").unwrap())
        .await
        .unwrap();
    assert_eq!(leaf_kids.len(), 2);

    // path_to from a leaf returns [root, section, leaf].
    let leaf = NodeId::root(&doc)
        .child("sec", "1")
        .unwrap()
        .child("leaf", "1")
        .unwrap();
    let path = adapter.path_to(&leaf).await.unwrap();
    assert_eq!(path.len(), 3);
    assert!(!path[0].is_leaf);
    assert!(path[2].is_leaf);
}
