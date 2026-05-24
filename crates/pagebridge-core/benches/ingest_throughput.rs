//! End-to-end ingest throughput across synthetic document sizes and
//! concurrency settings. Drives the tuning matrix in `docs/PERF.md`.
//!
//! Run with `cargo bench -p pagebridge-core --bench ingest_throughput`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
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

struct LatencyLlm {
    delay_ms: u64,
    #[allow(dead_code)]
    calls: AtomicU32,
}
impl LatencyLlm {
    fn new(delay_ms: u64) -> Self {
        Self {
            delay_ms,
            calls: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for LatencyLlm {
    fn name(&self) -> &'static str {
        "latency"
    }
    fn model(&self) -> &str {
        "lat-1"
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

fn synthetic_md(leaves: usize) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("# Doc\n\nIntro.\n\n");
    let sections = (leaves / 4).max(1);
    let paras = leaves / sections;
    for s in 0..sections {
        out.push_str(&format!("## Section {s}\n\n"));
        for p in 0..paras {
            out.push_str(&format!("Body {p} in section {s}.\n\n"));
        }
    }
    out.into_bytes()
}

fn ingest_once(rt: &Runtime, concurrency: u32, raw: &[u8], delay_ms: u64) {
    rt.block_on(async {
        let storage = Arc::new(MemoryAdapter::new());
        let llm = Arc::new(LatencyLlm::new(delay_ms));
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

fn bench_tuning_matrix(c: &mut Criterion) {
    let rt = Runtime::new().expect("rt");
    let mut group = c.benchmark_group("ingest_tuning_matrix");
    group.sample_size(20);
    // Three document sizes x three concurrency settings x one latency band
    // keeps the runtime tractable while still showing the scaling curve.
    for leaves in [10usize, 100, 500] {
        let raw = synthetic_md(leaves);
        group.throughput(Throughput::Elements(leaves as u64));
        for c_cnt in [1u32, 4, 16] {
            let id = format!("leaves={leaves}_conc={c_cnt}_lat=5ms");
            group.bench_with_input(BenchmarkId::from_parameter(&id), &c_cnt, |b, &c_cnt| {
                b.iter(|| ingest_once(&rt, c_cnt, &raw, 5));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_tuning_matrix);
criterion_main!(benches);
