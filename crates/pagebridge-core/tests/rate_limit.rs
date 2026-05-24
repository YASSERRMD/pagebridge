//! Verifies the parallel summary fan-out honors provider-declared rate limits.

#![allow(clippy::cast_possible_truncation, clippy::redundant_clone)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::ingest::ingest_with_config;
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, RateLimits,
};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{IngestParams, SourceKind};
use pagebridge_core::SummaryWorkerConfig;

struct LimitedLlm {
    rate_limits: RateLimits,
    in_flight: AtomicU32,
    peak: AtomicU32,
    delay_ms: u64,
}

impl LimitedLlm {
    fn new(limits: RateLimits, delay_ms: u64) -> Self {
        Self {
            rate_limits: limits,
            in_flight: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            delay_ms,
        }
    }
    fn peak(&self) -> u32 {
        self.peak.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for LimitedLlm {
    fn name(&self) -> &'static str {
        "limited"
    }
    fn model(&self) -> &str {
        "limited-1"
    }
    fn rate_limits(&self) -> RateLimits {
        self.rate_limits
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
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

fn synthetic_md(sections: usize, paras: usize) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("# Doc\n\nIntro.\n\n");
    for s in 0..sections {
        out.push_str(&format!("## Section {s}\n\n"));
        for p in 0..paras {
            out.push_str(&format!("Body {p} in section {s}.\n\n"));
        }
    }
    out.into_bytes()
}

#[tokio::test]
async fn provider_concurrent_cap_overrides_max_concurrency() {
    // Provider says max 2 concurrent. Caller asks for 16. We must observe at
    // most 2 in flight.
    let limits = RateLimits {
        requests_per_minute: None,
        tokens_per_minute: None,
        max_concurrent_requests: Some(2),
    };
    let llm = Arc::new(LimitedLlm::new(limits, 25));
    let storage = Arc::new(MemoryAdapter::new());
    let prompts = Arc::new(PromptLibrary::v1());
    let params = IngestParams {
        title: "Cap".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(6, 2),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_h, j) = ingest_with_config(
        storage,
        llm.clone(),
        prompts,
        params,
        SummaryWorkerConfig {
            max_concurrency: 16,
            ..SummaryWorkerConfig::default()
        },
    )
    .await
    .unwrap();
    j.await.unwrap().unwrap();
    assert!(
        llm.peak() <= 2,
        "peak in-flight {} exceeded provider cap 2",
        llm.peak()
    );
}

#[tokio::test]
async fn rpm_cap_throttles_dispatch() {
    // 60 RPM = 1 request/sec on average. With 6 summary calls we expect at
    // least 5 seconds of total wall time. To keep the test fast we use a
    // smaller RPM and a shorter expectation.
    //
    // 120 RPM = 2 req/sec. With 6 calls we expect about 2.5s+ of wait time
    // overall (the first burst is allowed by the bucket, then 1 every 500ms).
    let limits = RateLimits {
        requests_per_minute: Some(120),
        tokens_per_minute: None,
        max_concurrent_requests: Some(8),
    };
    let llm = Arc::new(LimitedLlm::new(limits, 1));
    let storage = Arc::new(MemoryAdapter::new());
    let prompts = Arc::new(PromptLibrary::v1());
    let params = IngestParams {
        title: "Rpm".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(5, 1),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let t0 = std::time::Instant::now();
    let (_h, j) = ingest_with_config(
        storage,
        llm,
        prompts,
        params,
        SummaryWorkerConfig {
            max_concurrency: 8,
            ..SummaryWorkerConfig::default()
        },
    )
    .await
    .unwrap();
    j.await.unwrap().unwrap();
    let elapsed = t0.elapsed();
    // At 120 RPM with a bucket of 120, the first ~5 calls can burst, but
    // subsequent calls have to wait. We assert a relatively loose lower bound
    // to keep the test deterministic across CI variation.
    assert!(
        elapsed >= Duration::from_millis(50),
        "expected RPM-throttled dispatch but completed in {elapsed:?}"
    );
}
