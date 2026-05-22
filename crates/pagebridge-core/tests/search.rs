//! Integration tests for the search engine.

#![allow(clippy::redundant_clone, clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::ingest::ingest;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::search::{navigate, synthesize_answer};
use pagebridge_core::trace::TraceBuilder;
use pagebridge_core::types::{IngestParams, NavigationConfig, SourceKind};

const SAMPLE_MD: &str = "# Carbon Policy\n\
\n\
## Section 1: Goals\n\
\n\
Net zero by 2050 is mandatory.\n\
\n\
## Section 2: Implementation Timeline\n\
\n\
Phase one launches in Q1 2026.\n\
Phase two completes in Q4 2027.\n";

async fn ingest_sample() -> Arc<MemoryAdapter> {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..20 {
        llm.push_json(serde_json::json!({
            "title": "T",
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }));
    }
    let prompts = Arc::new(PromptLibrary::v1());
    let params = IngestParams {
        title: "Carbon Policy".into(),
        source_kind: SourceKind::Markdown,
        raw_text: SAMPLE_MD.as_bytes().to_vec(),
        doc_id: Some(DocId::new("policy").unwrap()),
        user_metadata: Default::default(),
    };
    let (_, join) = ingest(storage.clone(), llm, prompts, params).await.unwrap();
    join.await.unwrap().unwrap();
    storage
}

#[tokio::test]
async fn navigate_runs_end_to_end() {
    let storage_arc = ingest_sample().await;
    let storage_dyn: Arc<dyn pagebridge_core::adapter::StorageAdapter> = storage_arc.clone();
    let echo_llm = Arc::new(EchoLlmProvider::new());
    // Provide a canned select_leaves action so navigation converges fast.
    let doc = DocId::new("policy").unwrap();
    let leaf_id = NodeId::root(&doc)
        .child("sec", "1")
        .unwrap()
        .child("leaf", "1")
        .unwrap();
    for _ in 0..20 {
        echo_llm.push_json(serde_json::json!({
            "action": "select_leaves",
            "node_ids": [leaf_id.as_str()],
        }));
    }
    let llm: Arc<dyn pagebridge_core::llm::LlmProvider> = echo_llm;
    let prompts = PromptLibrary::v1();
    let mut trace = TraceBuilder::new("timeline rollout");
    let nav = navigate(
        &storage_dyn,
        &llm,
        &prompts,
        "timeline rollout",
        NavigationConfig::default(),
        Some(&doc),
        &mut trace,
    )
    .await
    .unwrap();
    // Either the LLM-driven select_leaves landed or the BM25 fallback kicked in.
    assert!(!trace.data.steps.is_empty());
    let _ = nav;
}

#[tokio::test]
async fn synthesize_with_no_leaves_returns_polite_message() {
    let storage: Arc<dyn pagebridge_core::adapter::StorageAdapter> = Arc::new(MemoryAdapter::new());
    let llm: Arc<dyn pagebridge_core::llm::LlmProvider> = Arc::new(EchoLlmProvider::new());
    let prompts = PromptLibrary::v1();
    let mut trace = TraceBuilder::new("q");
    let ans = synthesize_answer(&storage, &llm, &prompts, "q", vec![], &mut trace)
        .await
        .unwrap();
    assert!(!ans.text.is_empty());
    assert!(ans.citations.is_empty());
}
