//! Verifies the skip-on-equivalent re-ingest fast path.

#![allow(clippy::cast_possible_truncation)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};
use pagebridge_core::types::{IngestParams, ReingestPrediction, SourceKind};
use pagebridge_core::{Pagebridge, PagebridgeOptions, SummaryWorkerConfig};

struct CountLlm {
    json_calls: AtomicU32,
}
impl CountLlm {
    fn new() -> Self {
        Self {
            json_calls: AtomicU32::new(0),
        }
    }
}
#[async_trait]
impl LlmProvider for CountLlm {
    fn name(&self) -> &'static str {
        "count"
    }
    fn model(&self) -> &str {
        "count-1"
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
        })
    }
    async fn complete_json(
        &self,
        _req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.json_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({"routing_summary":"r","summary":"s","keywords":[]}))
    }
}

fn md() -> Vec<u8> {
    "# Doc\n\n## Sec\n\nBody.\n".as_bytes().to_vec()
}

#[tokio::test]
async fn identical_reingest_is_skipped_under_100ms() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(CountLlm::new());
    let opts = PagebridgeOptions::new(storage.clone(), llm.clone()).with_summary_worker_config(
        SummaryWorkerConfig {
            max_concurrency: 4,
            ..SummaryWorkerConfig::default()
        },
    );
    let pb = Pagebridge::new_with(opts).await.unwrap();
    let params = IngestParams {
        title: "Skip Test".into(),
        source_kind: SourceKind::Markdown,
        raw_text: md(),
        doc_id: Some(pagebridge_core::DocId::new("doc-skip").unwrap()),
        user_metadata: std::collections::BTreeMap::default(),
    };
    let handle = pb
        .ingest_document_with_progress(params.clone())
        .await
        .unwrap();
    handle.wait().await.unwrap();
    let after_first = llm.json_calls.load(Ordering::SeqCst);
    assert!(after_first > 0);

    // Re-ingest with identical content. Fast path must skip all LLM work.
    let t0 = std::time::Instant::now();
    let handle2 = pb.ingest_document_with_progress(params.clone()).await.unwrap();
    handle2.wait().await.unwrap();
    let elapsed = t0.elapsed();
    let after_second = llm.json_calls.load(Ordering::SeqCst);
    assert_eq!(
        after_second, after_first,
        "skip-on-equivalent must trigger zero new LLM calls"
    );
    assert!(
        elapsed.as_millis() < 100,
        "skip-on-equivalent should complete in <100ms, took {elapsed:?}"
    );
}

#[tokio::test]
async fn would_reingest_change_reports_no_change() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(CountLlm::new());
    let pb = Pagebridge::new(storage, llm).await.unwrap();
    let params = IngestParams {
        title: "Pred".into(),
        source_kind: SourceKind::Markdown,
        raw_text: md(),
        doc_id: Some(pagebridge_core::DocId::new("doc-pred").unwrap()),
        user_metadata: std::collections::BTreeMap::default(),
    };
    let h = pb.ingest_document_with_progress(params.clone()).await.unwrap();
    h.wait().await.unwrap();
    let p = pb.would_reingest_change(&params).await.unwrap();
    assert!(matches!(p, ReingestPrediction::NoChange));
}

#[tokio::test]
async fn would_reingest_change_reports_full_change_for_unknown_doc() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(CountLlm::new());
    let pb = Pagebridge::new(storage, llm).await.unwrap();
    let params = IngestParams {
        title: "Pred".into(),
        source_kind: SourceKind::Markdown,
        raw_text: md(),
        doc_id: Some(pagebridge_core::DocId::new("doc-unknown").unwrap()),
        user_metadata: std::collections::BTreeMap::default(),
    };
    let p = pb.would_reingest_change(&params).await.unwrap();
    assert!(matches!(p, ReingestPrediction::FullChange));
}
