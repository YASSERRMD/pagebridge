//! Integration tests for the JSON-file adapter.

#![allow(clippy::redundant_clone, clippy::items_after_statements)]

use pagebridge_adapter_jsonfile::JsonFileAdapter;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::DocumentEntry;
use tempfile::tempdir;

fn root_rec(doc: &DocId) -> NodeRecord {
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

fn leaf_rec(doc: &DocId, i: u32, title: &str, kw: &[&str]) -> NodeRecord {
    let root = NodeId::root(doc);
    let sec = root.child("sec", "1").unwrap();
    NodeRecord {
        node_id: sec.child("leaf", &i.to_string()).unwrap(),
        doc_id: doc.clone(),
        parent_id: Some(sec),
        title: title.into(),
        level: NodeLevel::Leaf,
        routing_summary: format!("toc for {title}"),
        summary: format!("body of {title}"),
        child_ids: vec![],
        span: Some((0, 20)),
        page_start: None,
        page_end: None,
        keywords: kw.iter().map(|s| (*s).to_owned()).collect(),
        is_leaf: true,
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
        let adapter = JsonFileAdapter::open(&path).unwrap();
        adapter.migrate().await.unwrap();
        let doc = DocId::new("d1").unwrap();
        adapter.upsert_node(&root_rec(&doc)).await.unwrap();
        let sec_root = NodeId::root(&doc);
        let sec = sec_root.child("sec", "1").unwrap();
        adapter
            .upsert_node(&NodeRecord {
                node_id: sec,
                doc_id: doc.clone(),
                parent_id: Some(sec_root),
                title: "Section 1".into(),
                level: NodeLevel::Section,
                routing_summary: "toc 1".into(),
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
            })
            .await
            .unwrap();
        adapter
            .upsert_node(&leaf_rec(&doc, 1, "Timeline rollout", &["timeline"]))
            .await
            .unwrap();
        adapter
            .upsert_document(&DocumentEntry {
                doc_id: doc.clone(),
                title: "Doc".into(),
                source_kind: "markdown".into(),
                ingested_at: 1,
                root_node_id: NodeId::root(&doc),
                leaf_count: 1,
                byte_count: 0,
                raw_text_hash: None,
                structural_hash: None,
                document_type: None,
            })
            .await
            .unwrap();
        // Substring "BM25" works as a fallback.
        let hits = adapter.bm25_search("timeline", 5).await.unwrap();
        assert!(!hits.is_empty());
    }
    // Reopen.
    let adapter = JsonFileAdapter::open(&path).unwrap();
    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    let doc = DocId::new("d1").unwrap();
    let leaves = adapter.leaves_under(&NodeId::root(&doc)).await.unwrap();
    assert_eq!(leaves.len(), 1);
}

#[tokio::test]
async fn raw_and_summary_cache() {
    let dir = tempdir().unwrap();
    let adapter = JsonFileAdapter::open(dir.path()).unwrap();
    adapter.migrate().await.unwrap();
    let doc = DocId::new("d2").unwrap();
    adapter.put_raw(&doc, b"hello ").await.unwrap();
    adapter.put_raw(&doc, b"world").await.unwrap();
    let s = adapter.read_raw_text(&doc, (0, 11)).await.unwrap();
    assert_eq!(s, "hello world");

    let hash = [4u8; 32];
    assert!(adapter.get_summary_cache(&hash).await.unwrap().is_none());
    use pagebridge_core::types::SummaryCacheEntry;
    adapter
        .upsert_summary_cache(
            &hash,
            &SummaryCacheEntry {
                routing_summary: "rs".into(),
                summary: "s".into(),
                keywords: vec![],
                model_fingerprint: "m".into(),
                created_at: 0,
            },
        )
        .await
        .unwrap();
    assert!(adapter.get_summary_cache(&hash).await.unwrap().is_some());
}
