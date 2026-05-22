//! Quickstart: ingest a markdown blob into an in-memory store, ask a question,
//! and print the answer plus citations.

#![allow(clippy::default_trait_access)]
//!
//! This example uses the in-memory mock adapter and the deterministic mock LLM
//! provider so it runs with no external services. For a real LLM, swap in
//! [`pagebridge::OllamaProvider`] and the storage adapter of your choice.

use std::sync::Arc;

use pagebridge::{IngestParams, Pagebridge, SourceKind, StorageAdapter};
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;

const SAMPLE_MD: &str = "# Carbon Policy 2026\n\
\n\
## Section 1: Goals\n\
\n\
Net-zero by 2050.\n\
\n\
## Section 2: Implementation\n\
\n\
Phase one launches in Q1 2026.\n";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let storage: Arc<dyn StorageAdapter> = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    // Canned navigation: select a known leaf.
    for _ in 0..30 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let llm = echo;

    let bridge = Pagebridge::new(storage, llm).await?;
    let handle = bridge
        .ingest_document(IngestParams {
            title: "Carbon Policy 2026".into(),
            source_kind: SourceKind::Markdown,
            raw_text: SAMPLE_MD.as_bytes().to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await?;
    bridge.wait_for_summaries(&handle.doc_id).await?;

    let answer = bridge.ask("When does Phase one launch?").await?;
    println!("Answer: {}", answer.text);
    for c in &answer.citations {
        println!("  - {} ({})", c.section_title, c.node_id);
    }
    println!(
        "Trace: {} LLM calls, {} ms, {} input tokens, {} output tokens",
        answer.trace.total_llm_calls,
        answer.trace.duration_ms,
        answer.trace.total_input_tokens,
        answer.trace.total_output_tokens,
    );
    Ok(())
}
