//! Integration tests for the Phase J1 document type classifier.
//!
//! Covers three observable behaviors that downstream phases depend on:
//!   1. A high-confidence verdict round-trips into `DocumentEntry::document_type`.
//!   2. A low-confidence verdict collapses to `Generic` without poisoning
//!      downstream parsing decisions.
//!   3. A classifier provider error does NOT abort ingest; the document
//!      lands with `document_type = Some(Generic)`.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::unnecessary_literal_bound,
    clippy::struct_field_names
)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::ingest::{ingest_full, ClassifyConfig};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{DocumentType, IngestParams, SourceKind};
use pagebridge_core::StorageAdapter;
use pagebridge_core::SummaryWorkerConfig;

/// Mock provider that returns a scripted classifier verdict for the very
/// first `complete_json` call and a generic summarize-shaped object for
/// every subsequent call (so summary fan-out is exercised normally).
struct ScriptedLlm {
    classify_response: serde_json::Value,
    classify_calls: AtomicU32,
    classify_error: bool,
}

impl ScriptedLlm {
    fn ok(response: serde_json::Value) -> Self {
        Self {
            classify_response: response,
            classify_calls: AtomicU32::new(0),
            classify_error: false,
        }
    }

    fn err() -> Self {
        Self {
            classify_response: serde_json::Value::Null,
            classify_calls: AtomicU32::new(0),
            classify_error: true,
        }
    }

    fn classify_calls(&self) -> u32 {
        self.classify_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn model(&self) -> &str {
        "scripted-1"
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
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let is_classify = schema
            .get("properties")
            .and_then(|p| p.get("document_type"))
            .is_some();
        if is_classify {
            self.classify_calls.fetch_add(1, Ordering::SeqCst);
            if self.classify_error {
                return Err(PagebridgeError::Llm {
                    provider: "scripted".into(),
                    message: "synthetic classifier failure".into(),
                });
            }
            return Ok(self.classify_response.clone());
        }
        // Every other complete_json call is the summarize prompt; hand
        // back a benign shaped response so the summary fan-out completes.
        Ok(serde_json::json!({
            "title": "ok",
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

fn cv_markdown() -> Vec<u8> {
    "# John Doe\n\n## Skills\nRust, Python, Kubernetes.\n\n## Experience\nAcme 2020-2024.\n"
        .as_bytes()
        .to_vec()
}

async fn run_ingest(llm: Arc<ScriptedLlm>, classify: ClassifyConfig) -> Arc<MemoryAdapter> {
    let storage = Arc::new(MemoryAdapter::new());
    let prompts = Arc::new(PromptLibrary::v1());
    let params = IngestParams {
        title: "John Doe Resume".into(),
        source_kind: SourceKind::Markdown,
        raw_text: cv_markdown(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_handle, join) = ingest_full(
        storage.clone(),
        llm.clone(),
        prompts,
        params,
        SummaryWorkerConfig::default(),
        classify,
        None,
    )
    .await
    .unwrap();
    join.await.unwrap().unwrap();
    storage
}

#[tokio::test]
async fn classifier_happy_path_persists_resume_verdict() {
    let llm = Arc::new(ScriptedLlm::ok(serde_json::json!({
        "document_type": "resume",
        "confidence": 0.92,
        "reasons": ["bullets of work experience"]
    })));
    let storage = run_ingest(
        llm.clone(),
        ClassifyConfig {
            enabled: true,
            min_confidence: 0.5,
            sample_chars: 4000,
        },
    )
    .await;

    let docs = storage.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].document_type, Some(DocumentType::Resume));
    assert_eq!(
        llm.classify_calls(),
        1,
        "classifier must be invoked exactly once per ingest"
    );
}

#[tokio::test]
async fn classifier_low_confidence_collapses_to_generic() {
    let llm = Arc::new(ScriptedLlm::ok(serde_json::json!({
        "document_type": "resume",
        "confidence": 0.2
    })));
    let storage = run_ingest(
        llm.clone(),
        ClassifyConfig {
            enabled: true,
            min_confidence: 0.5,
            sample_chars: 4000,
        },
    )
    .await;

    let docs = storage.list_documents().await.unwrap();
    assert_eq!(docs[0].document_type, Some(DocumentType::Generic));
    assert_eq!(llm.classify_calls(), 1);
}

#[tokio::test]
async fn classifier_provider_error_does_not_abort_ingest() {
    let llm = Arc::new(ScriptedLlm::err());
    let storage = run_ingest(
        llm.clone(),
        ClassifyConfig {
            enabled: true,
            min_confidence: 0.5,
            sample_chars: 4000,
        },
    )
    .await;

    let docs = storage.list_documents().await.unwrap();
    assert_eq!(
        docs.len(),
        1,
        "ingest must complete despite classifier error"
    );
    assert_eq!(docs[0].document_type, Some(DocumentType::Generic));
    assert_eq!(llm.classify_calls(), 1);
}

#[tokio::test]
async fn classifier_disabled_skips_llm_call() {
    let llm = Arc::new(ScriptedLlm::ok(serde_json::json!({
        "document_type": "resume",
        "confidence": 0.99
    })));
    let storage = run_ingest(
        llm.clone(),
        ClassifyConfig {
            enabled: false,
            ..ClassifyConfig::default()
        },
    )
    .await;

    let docs = storage.list_documents().await.unwrap();
    assert_eq!(docs[0].document_type, None);
    assert_eq!(
        llm.classify_calls(),
        0,
        "classifier must not be called when disabled"
    );
}
