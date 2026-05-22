//! End-to-end facade test using MemoryAdapter + EchoLlmProvider.

#![allow(clippy::redundant_clone, clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{IngestParams, Pagebridge, SourceKind};

const SAMPLE: &str = "# Doc\n\n## Section 1\n\nTimeline rollout in Q1.\n";

#[tokio::test]
async fn facade_full_loop() {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    for _ in 0..30 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let bridge = Pagebridge::new(storage, echo).await.unwrap();
    let handle = bridge
        .ingest_document(IngestParams {
            title: "Doc".into(),
            source_kind: SourceKind::Markdown,
            raw_text: SAMPLE.as_bytes().to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&handle.doc_id).await.unwrap();
    let docs = bridge.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);

    let stats = bridge.stats().await.unwrap();
    assert_eq!(stats.adapter_name, "memory");

    let answer = bridge.ask("rollout").await.unwrap();
    assert!(!answer.text.is_empty());
    assert!(answer.trace.duration_ms == 0 || answer.trace.duration_ms < 60_000);

    bridge.remove_document(&handle.doc_id).await.unwrap();
    assert!(bridge.list_documents().await.unwrap().is_empty());
}
