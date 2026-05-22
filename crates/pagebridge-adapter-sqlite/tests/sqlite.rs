//! Integration tests for the SQLite adapter.

#![allow(clippy::redundant_clone)]

use pagebridge_adapter_sqlite::SqliteAdapter;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::{DocumentEntry, SummaryCacheEntry};
use tempfile::NamedTempFile;

fn make_root(doc: &DocId) -> NodeRecord {
    NodeRecord {
        node_id: NodeId::root(doc),
        doc_id: doc.clone(),
        parent_id: None,
        title: format!("Document {doc}"),
        level: NodeLevel::Document,
        routing_summary: "doc root".into(),
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
async fn roundtrip_and_fts() {
    let file = NamedTempFile::new().unwrap();
    let adapter = SqliteAdapter::open(file.path()).await.unwrap();
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

    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);

    let kids = adapter
        .children_summaries(&NodeId::root(&doc))
        .await
        .unwrap();
    assert_eq!(kids.len(), 1);

    let leaves = adapter.leaves_under(&NodeId::root(&doc)).await.unwrap();
    assert_eq!(leaves.len(), 2);

    let hits = adapter.bm25_search("timeline", 10).await.unwrap();
    assert!(!hits.is_empty(), "BM25 returned no hits");
    assert!(hits.iter().any(|h| h.title.contains("Timeline")));

    // Score normalization: positive (higher = better).
    assert!(hits.iter().all(|h| h.score >= 0.0));
}

#[tokio::test]
async fn raw_text_chunked() {
    let adapter = SqliteAdapter::memory().await.unwrap();
    adapter.migrate().await.unwrap();
    let doc = DocId::new("d2").unwrap();
    let payload = b"hello world, the quick brown fox jumps over the lazy dog. ";
    let mut total = 0usize;
    for _ in 0..2 {
        let written = adapter.put_raw(&doc, payload).await.unwrap();
        assert_eq!(written, total as u64);
        total += payload.len();
    }
    let text = adapter
        .read_raw_text(&doc, (0, total as u64))
        .await
        .unwrap();
    assert_eq!(text.len(), total);
    let span = adapter
        .read_raw_text(&doc, (payload.len() as u64 - 5, payload.len() as u64 + 5))
        .await
        .unwrap();
    assert_eq!(span.len(), 10);
}

#[tokio::test]
async fn summary_cache_roundtrip() {
    let adapter = SqliteAdapter::memory().await.unwrap();
    adapter.migrate().await.unwrap();
    let hash = [7u8; 32];
    assert!(adapter.get_summary_cache(&hash).await.unwrap().is_none());
    let entry = SummaryCacheEntry {
        routing_summary: "rs".into(),
        summary: "longer".into(),
        keywords: vec!["a".into(), "b".into()],
        model_fingerprint: "m".into(),
        created_at: 1,
    };
    adapter.upsert_summary_cache(&hash, &entry).await.unwrap();
    let back = adapter.get_summary_cache(&hash).await.unwrap().unwrap();
    assert_eq!(back.summary, "longer");
    assert_eq!(back.keywords, vec!["a".to_string(), "b".to_string()]);
}
