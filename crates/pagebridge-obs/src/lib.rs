//! Observability for pagebridge: Prometheus metrics and tracing helpers.
//!
//! Holds a process-wide `Registry` plus typed counters/gauges/histograms for
//! the operations pagebridge cares about (asks, ingests, LLM calls, adapter
//! ops). All instruments are best-effort: failure to register at startup is
//! logged and falls back to no-op metrics so callers never have to handle a
//! registration error.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::items_after_test_module
)]

use parking_lot::Mutex;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use std::sync::LazyLock;

/// Process-wide metric registry. Use [`registry`] to access it.
static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// Lazily-initialised metric handles.
static METRICS: LazyLock<Mutex<Option<Metrics>>> = LazyLock::new(|| Mutex::new(None));

/// Strongly-typed handles to every metric pagebridge emits.
#[derive(Clone)]
pub struct Metrics {
    pub asks_total: IntCounterVec,
    pub ask_duration_seconds: Histogram,
    pub ingest_duration_seconds: Histogram,
    pub llm_calls_total: IntCounterVec,
    pub llm_tokens_total: IntCounterVec,
    pub adapter_ops_total: IntCounterVec,
    pub active_documents: IntGauge,
    pub cache_hit_rate: Gauge,
    pub navigation_depth: Histogram,
}

impl Metrics {
    fn build() -> Result<Self, prometheus::Error> {
        let asks_total = IntCounterVec::new(
            Opts::new("pagebridge_asks_total", "Total ask() invocations"),
            &["outcome"],
        )?;
        let ask_duration_seconds = Histogram::with_opts(HistogramOpts::new(
            "pagebridge_ask_duration_seconds",
            "End-to-end ask() latency, in seconds",
        ))?;
        let ingest_duration_seconds = Histogram::with_opts(HistogramOpts::new(
            "pagebridge_ingest_duration_seconds",
            "Structural ingest duration, in seconds",
        ))?;
        let llm_calls_total = IntCounterVec::new(
            Opts::new("pagebridge_llm_calls_total", "Total LLM API calls"),
            &["provider", "model", "kind"],
        )?;
        let llm_tokens_total = IntCounterVec::new(
            Opts::new("pagebridge_llm_tokens_total", "LLM tokens processed"),
            &["provider", "model", "direction"],
        )?;
        let adapter_ops_total = IntCounterVec::new(
            Opts::new("pagebridge_adapter_ops_total", "Adapter operations"),
            &["adapter", "operation"],
        )?;
        let active_documents = IntGauge::new(
            "pagebridge_active_documents",
            "Number of ingested documents currently held",
        )?;
        let cache_hit_rate = Gauge::new(
            "pagebridge_summary_cache_hit_rate",
            "Hit rate of the summary cache (0.0 to 1.0)",
        )?;
        let navigation_depth = Histogram::with_opts(HistogramOpts::new(
            "pagebridge_navigation_depth",
            "Levels descended by the navigator on each ask",
        ))?;

        REGISTRY.register(Box::new(asks_total.clone()))?;
        REGISTRY.register(Box::new(ask_duration_seconds.clone()))?;
        REGISTRY.register(Box::new(ingest_duration_seconds.clone()))?;
        REGISTRY.register(Box::new(llm_calls_total.clone()))?;
        REGISTRY.register(Box::new(llm_tokens_total.clone()))?;
        REGISTRY.register(Box::new(adapter_ops_total.clone()))?;
        REGISTRY.register(Box::new(active_documents.clone()))?;
        REGISTRY.register(Box::new(cache_hit_rate.clone()))?;
        REGISTRY.register(Box::new(navigation_depth.clone()))?;

        Ok(Self {
            asks_total,
            ask_duration_seconds,
            ingest_duration_seconds,
            llm_calls_total,
            llm_tokens_total,
            adapter_ops_total,
            active_documents,
            cache_hit_rate,
            navigation_depth,
        })
    }
}

/// Initialise the metric registry. Idempotent: safe to call from `main` and
/// from any embedded server. Subsequent calls are no-ops.
pub fn init() {
    let mut guard = METRICS.lock();
    if guard.is_none() {
        match Metrics::build() {
            Ok(m) => *guard = Some(m),
            Err(e) => {
                tracing::warn!("pagebridge-obs metric registration failed: {e}");
            }
        }
    }
}

/// Access the metric handles. Returns `None` until [`init`] has been called.
#[must_use]
pub fn metrics() -> Option<Metrics> {
    METRICS.lock().clone()
}

/// Borrow the underlying Prometheus registry, e.g. for HTTP `/metrics`.
#[must_use]
pub fn registry() -> &'static Registry {
    &REGISTRY
}

/// Encode the registry as Prometheus text format.
pub fn encode_text() -> Result<String, prometheus::Error> {
    let mut buf = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    encoder.encode(&metric_families, &mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Convenience: increment an ask counter with the given outcome label.
pub fn record_ask(outcome: &str, duration_seconds: f64) {
    if let Some(m) = metrics() {
        m.asks_total.with_label_values(&[outcome]).inc();
        m.ask_duration_seconds.observe(duration_seconds);
    }
}

/// Convenience: record an LLM call and its token counts.
pub fn record_llm(provider: &str, model: &str, kind: &str, input: u64, output: u64) {
    let Some(m) = metrics() else { return };
    m.llm_calls_total
        .with_label_values(&[provider, model, kind])
        .inc();
    m.llm_tokens_total
        .with_label_values(&[provider, model, "input"])
        .inc_by(input);
    m.llm_tokens_total
        .with_label_values(&[provider, model, "output"])
        .inc_by(output);
}

/// Convenience: record an adapter operation.
pub fn record_adapter_op(adapter: &str, operation: &str) {
    if let Some(m) = metrics() {
        m.adapter_ops_total
            .with_label_values(&[adapter, operation])
            .inc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        init();
        init();
        let m = metrics().expect("metrics initialised");
        m.asks_total.with_label_values(&["ok"]).inc();
        let text = encode_text().expect("encode");
        assert!(text.contains("pagebridge_asks_total"));
    }

    #[test]
    fn record_helpers_increment_counters() {
        init();
        record_llm("ollama", "qwen2.5:7b", "complete", 100, 50);
        record_adapter_op("sqlite", "get_node");
        let text = encode_text().expect("encode");
        assert!(text.contains("pagebridge_llm_calls_total"));
        assert!(text.contains("pagebridge_adapter_ops_total"));
    }
}

/// Suppress dead-code lint for the unused Counter alias; kept exported because
/// downstream may reach in to register custom counters under the same registry.
#[allow(dead_code)]
type _ReExport = IntCounter;
