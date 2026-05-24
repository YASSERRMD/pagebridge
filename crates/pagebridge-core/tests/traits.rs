//! Trait object dispatch tests for `StorageAdapter` and `LlmProvider`.

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::llm::{ChatMessage, CompletionRequest, EchoLlmProvider};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::DocumentEntry;
use pagebridge_core::{LlmProvider, StorageAdapter};

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
        span: Some((0, 16)),
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
async fn memory_adapter_roundtrip() {
    let adapter: Arc<dyn StorageAdapter> = Arc::new(MemoryAdapter::new());
    adapter.migrate().await.unwrap();

    let doc = DocId::new("doc1").unwrap();
    let root = NodeId::root(&doc);

    // Document root + one section + two leaves.
    let root_rec = NodeRecord {
        node_id: root.clone(),
        doc_id: doc.clone(),
        parent_id: None,
        title: "Doc 1".into(),
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
    };
    adapter.upsert_node(&root_rec).await.unwrap();
    adapter
        .upsert_node(&make_section(&doc, 1, "Introduction"))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 1, "Intro one", &["timeline", "policy"]))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 2, "Intro two", &["budget"]))
        .await
        .unwrap();

    let kids = adapter.children_summaries(&root).await.unwrap();
    assert_eq!(kids.len(), 1);

    let leaves = adapter.leaves_under(&root).await.unwrap();
    assert_eq!(leaves.len(), 2);

    let hits = adapter.bm25_search("timeline policy", 10).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].title.contains("Intro one"));

    // Document entry round-trip.
    adapter
        .upsert_document(&DocumentEntry {
            doc_id: doc.clone(),
            title: "Doc 1".into(),
            source_kind: "markdown".into(),
            ingested_at: 1,
            root_node_id: root.clone(),
            leaf_count: 2,
            byte_count: 0,
            raw_text_hash: None,
            structural_hash: None,
        })
        .await
        .unwrap();
    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);

    // Raw text.
    let off = adapter.put_raw(&doc, b"hello world").await.unwrap();
    assert_eq!(off, 0);
    let text = adapter.read_raw_text(&doc, (0, 5)).await.unwrap();
    assert_eq!(text, "hello");

    // Delete cleans everything for the document.
    adapter.delete_document(&doc).await.unwrap();
    assert!(adapter.list_documents().await.unwrap().is_empty());
    assert!(adapter.children_summaries(&root).await.unwrap().is_empty());
}

#[tokio::test]
async fn echo_llm_provider_dispatch() {
    let llm: Arc<dyn LlmProvider> = Arc::new(EchoLlmProvider::new());
    assert_eq!(llm.name(), "echo");

    let resp = llm
        .complete(CompletionRequest::user("hello"))
        .await
        .unwrap();
    assert!(resp.text.contains("hello"));

    // Canned response takes priority.
    let prov = EchoLlmProvider::new();
    prov.push("canned response");
    let resp = prov
        .complete(CompletionRequest::user("anything"))
        .await
        .unwrap();
    assert_eq!(resp.text, "canned response");

    // JSON mode default returns empty object.
    let v = llm
        .complete_json(
            CompletionRequest {
                messages: vec![ChatMessage::user("give me json")],
                ..Default::default()
            },
            &serde_json::json!({}),
        )
        .await
        .unwrap();
    assert!(v.is_object());

    // Canned JSON takes priority.
    let prov2 = EchoLlmProvider::new();
    prov2.push_json(serde_json::json!({"action": "descend"}));
    let v = prov2
        .complete_json(CompletionRequest::user("q"), &serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(v["action"], "descend");
}
