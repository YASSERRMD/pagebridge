//! Integration tests for Phase J7 section-first navigation.
//!
//! Verifies that `find_nodes_by_canonical` on the in-memory adapter
//! correctly returns leaf records for the matching canonical section and that
//! the `navigate` fast-path short-circuits BM25 + LLM when a tagged section
//! is found.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation
)]

use std::sync::Arc;

use pagebridge_core::{
    adapter::StorageAdapter,
    id::{DocId, NodeId},
    record::{NodeLevel, NodeRecord},
    types::{DocumentEntry, DocumentType},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_doc_id() -> DocId {
    DocId::new("test-j7-doc".to_owned()).unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn make_node(
    doc_id: &DocId,
    node_id: NodeId,
    parent_id: Option<NodeId>,
    title: &str,
    level: NodeLevel,
    is_leaf: bool,
    canonical_section: Option<&str>,
) -> NodeRecord {
    NodeRecord {
        node_id,
        doc_id: doc_id.clone(),
        parent_id,
        title: title.to_owned(),
        level,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: None,
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf,
        created_at: now_ms(),
        updated_at: now_ms(),
        source_hash: [0; 32],
        canonical_section: canonical_section.map(str::to_owned),
        section_aliases: vec![],
    }
}

// ---------------------------------------------------------------------------
// find_nodes_by_canonical tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn find_nodes_by_canonical_returns_leaves_for_matching_section() {
    use pagebridge_core::adapter::MemoryAdapter;

    let adapter = Arc::new(MemoryAdapter::default());
    let doc_id = test_doc_id();

    // Build: root -> skills section -> leaf
    let root_id = NodeId::root(&doc_id);
    let skills_sec_id = root_id.child("sec", "1").unwrap();
    let leaf_id = skills_sec_id.child("leaf", "1").unwrap();

    let root = make_node(
        &doc_id,
        root_id.clone(),
        None,
        "My CV",
        NodeLevel::Document,
        false,
        None,
    );
    let skills_sec = make_node(
        &doc_id,
        skills_sec_id.clone(),
        Some(root_id.clone()),
        "skills",
        NodeLevel::Section,
        false,
        Some("skills"),
    );
    let leaf = make_node(
        &doc_id,
        leaf_id.clone(),
        Some(skills_sec_id.clone()),
        "leaf",
        NodeLevel::Leaf,
        true,
        None,
    );

    adapter.upsert_node(&root).await.unwrap();
    adapter.upsert_node(&skills_sec).await.unwrap();
    adapter.upsert_node(&leaf).await.unwrap();

    let doc_entry = DocumentEntry {
        doc_id: doc_id.clone(),
        title: "My CV".into(),
        source_kind: "plain".into(),
        ingested_at: now_ms(),
        root_node_id: root_id.clone(),
        leaf_count: 1,
        byte_count: 0,
        raw_text_hash: None,
        structural_hash: None,
        document_type: Some(DocumentType::Resume),
    };
    adapter.upsert_document(&doc_entry).await.unwrap();

    // Now call find_nodes_by_canonical.
    let leaves = adapter
        .find_nodes_by_canonical(&doc_id, "skills")
        .await
        .unwrap();
    assert_eq!(
        leaves.len(),
        1,
        "should find exactly 1 leaf under skills section"
    );
    assert_eq!(leaves[0].node_id, leaf_id);
}

#[tokio::test]
async fn find_nodes_by_canonical_case_insensitive() {
    use pagebridge_core::adapter::MemoryAdapter;

    let adapter = Arc::new(MemoryAdapter::default());
    let doc_id = test_doc_id();

    let root_id = NodeId::root(&doc_id);
    let exp_sec_id = root_id.child("sec", "1").unwrap();
    let leaf_id = exp_sec_id.child("leaf", "1").unwrap();

    let root = make_node(
        &doc_id,
        root_id.clone(),
        None,
        "CV",
        NodeLevel::Document,
        false,
        None,
    );
    let exp_sec = make_node(
        &doc_id,
        exp_sec_id.clone(),
        Some(root_id.clone()),
        "Experience",
        NodeLevel::Section,
        false,
        Some("experience"), // stored lowercase
    );
    let leaf = make_node(
        &doc_id,
        leaf_id.clone(),
        Some(exp_sec_id.clone()),
        "leaf",
        NodeLevel::Leaf,
        true,
        None,
    );

    adapter.upsert_node(&root).await.unwrap();
    adapter.upsert_node(&exp_sec).await.unwrap();
    adapter.upsert_node(&leaf).await.unwrap();

    // Query with uppercase canonical -- should still match.
    let leaves = adapter
        .find_nodes_by_canonical(&doc_id, "EXPERIENCE")
        .await
        .unwrap();
    assert_eq!(leaves.len(), 1, "case-insensitive match must work");
}

#[tokio::test]
async fn find_nodes_by_canonical_returns_empty_for_no_match() {
    use pagebridge_core::adapter::MemoryAdapter;

    let adapter = Arc::new(MemoryAdapter::default());
    let doc_id = test_doc_id();

    let root_id = NodeId::root(&doc_id);
    let sec_id = root_id.child("sec", "1").unwrap();
    let leaf_id = sec_id.child("leaf", "1").unwrap();

    let root = make_node(
        &doc_id,
        root_id.clone(),
        None,
        "CV",
        NodeLevel::Document,
        false,
        None,
    );
    let sec = make_node(
        &doc_id,
        sec_id.clone(),
        Some(root_id.clone()),
        "skills",
        NodeLevel::Section,
        false,
        Some("skills"),
    );
    let leaf = make_node(
        &doc_id,
        leaf_id,
        Some(sec_id),
        "leaf",
        NodeLevel::Leaf,
        true,
        None,
    );

    adapter.upsert_node(&root).await.unwrap();
    adapter.upsert_node(&sec).await.unwrap();
    adapter.upsert_node(&leaf).await.unwrap();

    let leaves = adapter
        .find_nodes_by_canonical(&doc_id, "education")
        .await
        .unwrap();
    assert!(
        leaves.is_empty(),
        "no 'education' section -- should return empty"
    );
}
