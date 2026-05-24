//! End-to-end streaming test: ingest, then ask_stream and assert citations
//! and text chunks come through in the expected order.

#![allow(clippy::redundant_clone, clippy::default_trait_access)]

use std::sync::Arc;

use futures::StreamExt;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::{EchoLlmProvider, FinishReason, StreamChunk};
use pagebridge_core::{AnswerChunk, IngestParams, Pagebridge, SourceKind};

const SAMPLE: &str = "# Doc\n\n## Section 1\n\nTimeline rollout in Q1.\n";

#[tokio::test]
async fn ask_stream_emits_tokens_and_citations() {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    // Summarization JSON for ingest.
    for _ in 0..30 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let bridge = Pagebridge::new(storage, echo.clone()).await.unwrap();
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

    // Find a real leaf id under the new document so the citation marker
    // resolves cleanly.
    let leaves = bridge
        .storage()
        .leaves_under(&handle.root_node_id)
        .await
        .unwrap();
    assert!(!leaves.is_empty());
    let leaf_id = leaves[0].as_str();

    // Script the navigation JSON and the synthesis stream. The navigator may
    // be invoked multiple times depending on tree shape, so push enough.
    for _ in 0..16 {
        echo.push_json(serde_json::json!({
            "action": "select",
            "node_ids": [leaf_id],
            "reason": "test"
        }));
    }
    echo.push_stream(vec![
        StreamChunk::Token("Rollout is in Q1. ".into()),
        StreamChunk::Token(format!("See [[CITE:{leaf_id}]] for detail.")),
        StreamChunk::Finished {
            input_tokens: 5,
            output_tokens: 10,
            finish_reason: FinishReason::Stop,
        },
    ]);

    let mut stream = bridge.ask_stream("when is rollout?").await.unwrap();
    let mut tokens = String::new();
    let mut citation_count = 0;
    let mut saw_done = false;
    while let Some(chunk) = stream.next().await {
        match chunk.unwrap() {
            AnswerChunk::Token { text } => tokens.push_str(&text),
            AnswerChunk::Citation { citation } => {
                assert_eq!(citation.node_id.as_str(), leaf_id);
                citation_count += 1;
            }
            AnswerChunk::Done { citations, .. } => {
                saw_done = true;
                assert!(!citations.is_empty());
            }
        }
    }
    assert!(saw_done, "stream ended without Done chunk");
    assert!(citation_count >= 1, "expected at least one citation event");
    assert!(tokens.contains("Rollout"), "tokens missing: {tokens}");
    assert!(
        !tokens.contains("[[CITE:"),
        "marker leaked into text: {tokens}"
    );
}
