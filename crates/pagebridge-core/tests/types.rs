//! Integration tests for core types.

#![allow(clippy::redundant_clone)]

use pagebridge_core::error::PagebridgeError;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::Answer;
use std::str::FromStr;

#[test]
fn doc_id_validates() {
    assert!(DocId::new("carbon-policy-2026").is_ok());
    assert!(DocId::new("a").is_ok());
    assert!(DocId::new("under_score-and-dash-123").is_ok());

    assert!(matches!(
        DocId::new(""),
        Err(PagebridgeError::InvalidDocId(_))
    ));
    assert!(matches!(
        DocId::new("Has-Uppercase"),
        Err(PagebridgeError::InvalidDocId(_))
    ));
    assert!(matches!(
        DocId::new("space inside"),
        Err(PagebridgeError::InvalidDocId(_))
    ));
    let too_long = "x".repeat(65);
    assert!(matches!(
        DocId::new(too_long),
        Err(PagebridgeError::InvalidDocId(_))
    ));
}

#[test]
fn node_id_roundtrip_and_navigation() {
    let doc = DocId::new("carbon-policy-2026").unwrap();
    let root = NodeId::root(&doc);
    assert_eq!(root.as_str(), "doc:carbon-policy-2026");
    assert_eq!(root.depth(), 0);
    assert!(root.parent().is_none());

    let sec = root.child("sec", "1.2").unwrap();
    assert_eq!(sec.as_str(), "doc:carbon-policy-2026/sec:1.2");
    assert_eq!(sec.depth(), 1);
    assert_eq!(sec.parent().unwrap(), root);

    let leaf = sec.child("leaf", "7").unwrap();
    assert_eq!(leaf.depth(), 2);
    assert_eq!(leaf.doc_id().unwrap(), doc);
    assert!(root.is_prefix_of(&leaf));
    assert!(sec.is_prefix_of(&leaf));
    assert!(!leaf.is_prefix_of(&sec));

    let parsed = NodeId::from_str("doc:carbon-policy-2026/sec:1.2/leaf:7").unwrap();
    assert_eq!(parsed, leaf);
}

#[test]
fn node_id_rejects_garbage() {
    assert!(NodeId::new("no-doc-prefix").is_err());
    assert!(NodeId::new("doc:CapitalDoc").is_err());
    assert!(NodeId::new("doc:ok/segment-without-colon").is_err());
}

#[test]
fn node_record_validate_invariants() {
    let doc = DocId::new("d1").unwrap();
    let root = NodeId::root(&doc);
    let sec = root.child("sec", "1").unwrap();
    let leaf = sec.child("leaf", "a").unwrap();

    // Valid leaf.
    let r = NodeRecord {
        node_id: leaf.clone(),
        doc_id: doc.clone(),
        parent_id: Some(sec.clone()),
        title: "Intro".into(),
        level: NodeLevel::Leaf,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: Some((0, 12)),
        page_start: Some(1),
        page_end: Some(1),
        keywords: vec![],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    };
    assert!(r.validate().is_ok());

    // Empty title.
    let mut bad = r.clone();
    bad.title = "   ".into();
    assert!(bad.validate().is_err());

    // Leaf with children.
    let mut bad = r.clone();
    bad.child_ids = vec![leaf.clone()];
    assert!(bad.validate().is_err());

    // Span start > end.
    let mut bad = r.clone();
    bad.span = Some((10, 5));
    assert!(bad.validate().is_err());

    // Parent not a prefix.
    let mut bad = r.clone();
    bad.parent_id = Some(NodeId::root(&DocId::new("other").unwrap()));
    assert!(bad.validate().is_err());
}

#[test]
fn node_record_serde_roundtrip() {
    let doc = DocId::new("d1").unwrap();
    let r = NodeRecord {
        node_id: NodeId::root(&doc).child("leaf", "1").unwrap(),
        doc_id: doc,
        parent_id: Some(NodeId::root(&DocId::new("d1").unwrap())),
        title: "Hello".into(),
        level: NodeLevel::Leaf,
        routing_summary: "a brief line".into(),
        summary: "longer summary".into(),
        child_ids: vec![],
        span: Some((0, 5)),
        page_start: None,
        page_end: None,
        keywords: vec!["a".into(), "b".into()],
        is_leaf: true,
        created_at: 42,
        updated_at: 42,
        source_hash: [1; 32],
    };
    let s = serde_json::to_string(&r).unwrap();
    let back: NodeRecord = serde_json::from_str(&s).unwrap();
    assert_eq!(r, back);
}

#[test]
fn answer_serde_roundtrip() {
    let trace = pagebridge_core::types::QueryTrace {
        query_id: "abc".into(),
        question: "q".into(),
        started_at: 0,
        finished_at: 1,
        duration_ms: 1,
        steps: vec![],
        total_llm_calls: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        bm25_candidates: vec![],
        selected_leaves: vec![],
        final_citations: vec![],
    };
    let a = Answer {
        text: "ok".into(),
        citations: vec![],
        trace,
        receipt_json: None,
    };
    let s = serde_json::to_string(&a).unwrap();
    let back: Answer = serde_json::from_str(&s).unwrap();
    assert_eq!(a.text, back.text);
}

#[test]
fn unicode_safe_ids() {
    // Identifiers are intentionally ASCII-only; raw text and titles can be unicode.
    assert!(DocId::new("emoji-🚀").is_err());
}
