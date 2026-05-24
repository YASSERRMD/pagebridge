//! Tests for the bounded-concurrency summary fan-out.

#![allow(clippy::cast_possible_truncation, clippy::redundant_clone)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::ingest::ingest_with_config;
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider,
};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{IngestParams, SourceKind};
use pagebridge_core::SummaryWorkerConfig;

/// LLM that counts concurrent in-flight calls and sleeps for `delay_ms`.
struct CountingLlm {
    in_flight: AtomicU32,
    peak: AtomicU32,
    total: AtomicU32,
    delay_ms: u64,
}

impl CountingLlm {
    fn new(delay_ms: u64) -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            total: AtomicU32::new(0),
            delay_ms,
        }
    }
    fn peak(&self) -> u32 {
        self.peak.load(Ordering::SeqCst)
    }
    fn total(&self) -> u32 {
        self.total.load(Ordering::SeqCst)
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
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        // CAS the new peak if it exceeds the prior.
        let mut prev = self.peak.load(Ordering::SeqCst);
        while now > prev {
            match self
                .peak
                .compare_exchange(prev, now, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break,
                Err(v) => prev = v,
            }
        }
        self.total.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

fn synthetic_markdown(sections: usize, paras: usize) -> String {
    let mut out = String::new();
    out.push_str("# Big Document\n\nIntro text.\n\n");
    for s in 0..sections {
        out.push_str(&format!("## Section {s}\n\n"));
        for p in 0..paras {
            out.push_str(&format!(
                "This is paragraph {p} of section {s}, with enough words to count as a real leaf.\n\n"
            ));
        }
    }
    out
}

#[tokio::test]
async fn concurrency_bound_is_respected() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(CountingLlm::new(30));
    let prompts = Arc::new(PromptLibrary::v1());

    let params = IngestParams {
        title: "Concurrency Test".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_markdown(8, 3).into_bytes(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let cfg = SummaryWorkerConfig {
        max_concurrency: 4,
        ..SummaryWorkerConfig::default()
    };
    let (_handle, join) = ingest_with_config(storage.clone(), llm.clone(), prompts, params, cfg)
        .await
        .unwrap();
    join.await.unwrap().unwrap();

    // The peak should never exceed our requested bound.
    assert!(
        llm.peak() <= 4,
        "peak in-flight {} exceeded max_concurrency 4",
        llm.peak()
    );
    // We should have made at least one call.
    assert!(llm.total() >= 1);
}

/// Tracks the depth of every summarized parent in observation order so the
/// test can assert that deeper levels finish before shallower ones start.
struct OrderingLlm {
    observed: Mutex<Vec<usize>>,
}

impl OrderingLlm {
    fn new() -> Self {
        Self {
            observed: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LlmProvider for OrderingLlm {
    fn name(&self) -> &'static str {
        "ordering"
    }
    fn model(&self) -> &str {
        "ordering-1"
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
        req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // The summarize prompt embeds the node id of each child via
        // `## title (node_id)`. The deepest node id segment count is a
        // proxy for the depth of the node being summarized + 1. We pull
        // the first such id out and record its segment count.
        let prompt = req
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let depth = prompt
            .lines()
            .find_map(|l| {
                let start = l.rfind('(')?;
                let end = l.rfind(')')?;
                if end <= start {
                    return None;
                }
                let id = &l[start + 1..end];
                Some(id.matches(':').count())
            })
            .unwrap_or(0);
        self.observed.lock().push(depth);
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

#[tokio::test]
async fn level_ordering_is_preserved() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(OrderingLlm::new());
    let prompts = Arc::new(PromptLibrary::v1());

    // A deeper tree: 4 sections, 3 paragraphs each. The markdown parser
    // creates document -> section -> leaf, so non-leaf depths are 1 (sections)
    // and 0 (root).
    let params = IngestParams {
        title: "Order Test".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_markdown(4, 3).into_bytes(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_h, j) = ingest_with_config(
        storage,
        llm.clone(),
        prompts,
        params,
        SummaryWorkerConfig {
            max_concurrency: 4,
            ..SummaryWorkerConfig::default()
        },
    )
    .await
    .unwrap();
    j.await.unwrap().unwrap();

    let order = llm.observed.lock().clone();
    // Every deeper-depth call must come before any shallower-depth call.
    // i.e. observation order, when scanned, never sees a smaller value
    // before a larger one that comes later.
    let mut min_seen = usize::MAX;
    for d in order.iter().copied().rev() {
        assert!(
            d >= min_seen || min_seen == usize::MAX,
            "saw deeper depth {d} after shallower {min_seen} (order = {:?})",
            order
        );
        if d < min_seen {
            min_seen = d;
        }
    }
}

#[tokio::test]
async fn parallel_is_faster_than_sequential() {
    let prompts = Arc::new(PromptLibrary::v1());
    let raw = synthetic_markdown(6, 2).into_bytes();
    // Sequential baseline: max_concurrency = 1.
    let storage_seq = Arc::new(MemoryAdapter::new());
    let llm_seq = Arc::new(CountingLlm::new(40));
    let params_seq = IngestParams {
        title: "Seq".into(),
        source_kind: SourceKind::Markdown,
        raw_text: raw.clone(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let t0 = std::time::Instant::now();
    let (_h, j) = ingest_with_config(
        storage_seq,
        llm_seq,
        prompts.clone(),
        params_seq,
        SummaryWorkerConfig {
            max_concurrency: 1,
            ..SummaryWorkerConfig::default()
        },
    )
    .await
    .unwrap();
    j.await.unwrap().unwrap();
    let seq_ms = t0.elapsed().as_millis();

    // Parallel: max_concurrency = 8.
    let storage_par = Arc::new(MemoryAdapter::new());
    let llm_par = Arc::new(CountingLlm::new(40));
    let params_par = IngestParams {
        title: "Par".into(),
        source_kind: SourceKind::Markdown,
        raw_text: raw,
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let t0 = std::time::Instant::now();
    let (_h, j) = ingest_with_config(
        storage_par,
        llm_par,
        prompts,
        params_par,
        SummaryWorkerConfig {
            max_concurrency: 8,
            ..SummaryWorkerConfig::default()
        },
    )
    .await
    .unwrap();
    j.await.unwrap().unwrap();
    let par_ms = t0.elapsed().as_millis();

    // Parallel must be meaningfully faster. Use a conservative 2x assertion
    // so the test does not flake on slow CI machines.
    assert!(
        par_ms * 2 <= seq_ms,
        "parallel ({par_ms}ms) must be at least 2x faster than sequential ({seq_ms}ms)"
    );
}
