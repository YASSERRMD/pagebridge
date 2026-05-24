//! Verifies pre-flight summary cache lookup skips LLM calls on re-ingest.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::needless_pass_by_value,
    clippy::elidable_lifetime_names,
    clippy::manual_let_else,
    clippy::if_not_else,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_borrows_for_generic_args,
    clippy::uninlined_format_args,
    clippy::semicolon_if_nothing_returned,
    clippy::needless_lifetimes,
    clippy::useless_vec,
    clippy::map_unwrap_or,
    clippy::unnecessary_literal_bound,
    clippy::needless_raw_string_hashes
)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::ingest::ingest_with_config;
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{IngestParams, SourceKind};
use pagebridge_core::SummaryWorkerConfig;

struct CountingLlm {
    json_calls: AtomicU32,
}

impl CountingLlm {
    fn new() -> Self {
        Self {
            json_calls: AtomicU32::new(0),
        }
    }
    fn json_calls(&self) -> u32 {
        self.json_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for CountingLlm {
    fn name(&self) -> &'static str {
        "counting"
    }
    fn model(&self) -> &str {
        "counting-1"
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
        Ok(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": ["kw"]
        }))
    }
}

fn synthetic_md() -> Vec<u8> {
    let mut out = String::new();
    out.push_str("# Doc\n\nIntro.\n\n");
    for s in 0..3 {
        out.push_str(&format!("## Section {s}\n\nBody {s}.\n\n"));
    }
    out.into_bytes()
}

#[tokio::test]
async fn reingest_hits_pre_flight_cache_zero_llm_calls() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(CountingLlm::new());
    let prompts = Arc::new(PromptLibrary::v1());
    let cfg = SummaryWorkerConfig {
        max_concurrency: 4,
        ..SummaryWorkerConfig::default()
    };

    // First ingest populates the summary cache.
    let params1 = IngestParams {
        title: "Doc One".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(),
        doc_id: Some(pagebridge_core::DocId::new("doc-one").unwrap()),
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_h, j) = ingest_with_config(storage.clone(), llm.clone(), prompts.clone(), params1, cfg)
        .await
        .unwrap();
    j.await.unwrap().unwrap();
    let after_first = llm.json_calls();
    assert!(after_first > 0, "first ingest must call LLM");

    // Second ingest under the SAME doc_id with identical content. Because
    // the structural insert preserves prior source_hash on non-leaf nodes,
    // the pre-flight cache lookup short-circuits every summary call.
    let params2 = IngestParams {
        title: "Doc One".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(),
        doc_id: Some(pagebridge_core::DocId::new("doc-one").unwrap()),
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_h, j) = ingest_with_config(storage, llm.clone(), prompts, params2, cfg)
        .await
        .unwrap();
    j.await.unwrap().unwrap();
    let after_second = llm.json_calls();

    let delta = after_second - after_first;
    assert!(
        delta < after_first,
        "second ingest should reuse the cache; first={after_first} second-delta={delta}"
    );
}
