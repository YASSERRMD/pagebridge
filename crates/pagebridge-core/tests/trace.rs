//! Tests for query trace completeness and serialization.

#![allow(clippy::redundant_clone, clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::ingest::ingest;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::search::{navigate, synthesize_answer};
use pagebridge_core::trace::{render_citation_list, TraceBuilder};
use pagebridge_core::types::{IngestParams, NavigationConfig, SourceKind, TraceStep};

const SAMPLE_MD: &str = "# Doc\n\
\n\
## Section 1\n\
\n\
Apple banana cherry.\n\
\n\
## Section 2\n\
\n\
Implementation timeline.\n";

async fn fixture() -> (Arc<MemoryAdapter>, DocId) {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..20 {
        llm.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": ["timeline"]
        }));
    }
    let prompts = Arc::new(PromptLibrary::v1());
    let doc = DocId::new("doc1").unwrap();
    let params = IngestParams {
        title: "Doc".into(),
        source_kind: SourceKind::Markdown,
        raw_text: SAMPLE_MD.as_bytes().to_vec(),
        doc_id: Some(doc.clone()),
        user_metadata: Default::default(),
    };
    let (_, join) = ingest(storage.clone(), llm, prompts, params).await.unwrap();
    join.await.unwrap().unwrap();
    (storage, doc)
}

#[tokio::test]
async fn trace_contains_required_steps() {
    let (storage_arc, doc) = fixture().await;
    let storage: Arc<dyn pagebridge_core::adapter::StorageAdapter> = storage_arc.clone();
    let echo = Arc::new(EchoLlmProvider::new());
    let leaf = NodeId::root(&doc)
        .child("sec", "2")
        .unwrap()
        .child("leaf", "2")
        .unwrap();
    for _ in 0..10 {
        echo.push_json(serde_json::json!({"action":"select_leaves","node_ids":[leaf.as_str()]}));
    }
    let llm: Arc<dyn pagebridge_core::llm::LlmProvider> = echo;
    let prompts = PromptLibrary::v1();
    let mut trace = TraceBuilder::new("timeline");
    let nav = navigate(
        &storage,
        &llm,
        &prompts,
        "timeline",
        NavigationConfig::default(),
        Some(&doc),
        &mut trace,
    )
    .await
    .unwrap();

    let answer = synthesize_answer(
        &storage,
        &llm,
        &prompts,
        "timeline",
        nav.selected_leaves,
        &mut trace,
    )
    .await
    .unwrap();

    // Every trace should have a BM25Candidates step and a LeafSelection step.
    let has_bm25 = answer
        .trace
        .steps
        .iter()
        .any(|s| matches!(s, TraceStep::Bm25Candidates { .. }));
    let has_leaf_sel = answer
        .trace
        .steps
        .iter()
        .any(|s| matches!(s, TraceStep::LeafSelection { .. }));
    assert!(has_bm25, "trace missing BM25Candidates");
    assert!(has_leaf_sel, "trace missing LeafSelection");
    assert!(!answer.text.is_empty());
}

#[test]
fn trace_serializes_as_json() {
    let mut trace = TraceBuilder::new("hello?");
    trace.bm25(&[], 0.0);
    trace.finish();
    let json = serde_json::to_string(&trace.data).unwrap();
    let back: pagebridge_core::types::QueryTrace = serde_json::from_str(&json).unwrap();
    assert_eq!(back.question, "hello?");
}

#[test]
fn render_helper_shapes_humans() {
    let cit = pagebridge_core::types::Citation {
        node_id: NodeId::new("doc:x/leaf:1").unwrap(),
        doc_id: DocId::new("x").unwrap(),
        doc_title: "Doc X".into(),
        section_title: "Section 1".into(),
        page_range: Some((1, 2)),
        excerpt: "short".into(),
    };
    let s = render_citation_list(&[cit]);
    assert!(s.contains("Section 1"));
    assert!(s.contains("doc:x/leaf:1"));
}
