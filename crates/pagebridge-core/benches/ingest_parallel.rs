//! Microbenchmark: parallel vs sequential summary fan-out.
//!
//! Run with `cargo bench -p pagebridge-core --bench ingest_parallel`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::ingest::ingest_with_config;
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider,
};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{IngestParams, SourceKind};
use pagebridge_core::SummaryWorkerConfig;
use tokio::runtime::Runtime;

struct DelayLlm {
    in_flight: AtomicU32,
    peak: AtomicU32,
    delay_ms: u64,
}

impl DelayLlm {
    fn new(delay_ms: u64) -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            delay_ms,
        }
    }
}

#[async_trait]
impl LlmProvider for DelayLlm {
    fn name(&self) -> &'static str {
        "delay"
    }
    fn model(&self) -> &str {
        "delay-1"
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
        let n = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        let mut prev = self.peak.load(Ordering::SeqCst);
        while n > prev {
            match self
                .peak
                .compare_exchange(prev, n, Ordering::SeqCst, Ordering::SeqCst)
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

fn synthetic_markdown(sections: usize, paras: usize) -> String {
    let mut out = String::new();
    out.push_str("# Bench Doc\n\nIntro.\n\n");
    for s in 0..sections {
        out.push_str(&format!("## Section {s}\n\n"));
        for p in 0..paras {
            out.push_str(&format!(
                "Body paragraph {p} in section {s} with several words.\n\n"
            ));
        }
    }
    out
}

fn ingest_with(rt: &Runtime, concurrency: u32, raw: &[u8], delay_ms: u64) {
    rt.block_on(async {
        let storage = Arc::new(MemoryAdapter::new());
        let llm = Arc::new(DelayLlm::new(delay_ms));
        let prompts = Arc::new(PromptLibrary::v1());
        let params = IngestParams {
            title: "Bench".into(),
            source_kind: SourceKind::Markdown,
            raw_text: raw.to_vec(),
            doc_id: None,
            user_metadata: std::collections::BTreeMap::default(),
        };
        let (_h, j) = ingest_with_config(
            storage,
            llm,
            prompts,
            params,
            SummaryWorkerConfig {
                max_concurrency: concurrency,
                ..SummaryWorkerConfig::default()
            },
        )
        .await
        .expect("ingest");
        j.await.expect("join").expect("summaries");
    });
}

fn bench_parallel_vs_sequential(c: &mut Criterion) {
    let rt = Runtime::new().expect("rt");
    let raw = synthetic_markdown(6, 2).into_bytes();
    let mut group = c.benchmark_group("ingest_summary_fanout");
    for c_cnt in [1u32, 4, 8] {
        group.bench_with_input(BenchmarkId::from_parameter(c_cnt), &c_cnt, |b, &c_cnt| {
            b.iter(|| ingest_with(&rt, c_cnt, &raw, 5));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parallel_vs_sequential);
criterion_main!(benches);
