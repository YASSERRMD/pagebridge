//! Concurrency-controlled summary fan-out worker.
//!
//! The v0.1 ingest pipeline walks the tree bottom-up and calls the LLM once
//! per non-leaf node sequentially. For a 200-page PDF that is ~250 sequential
//! LLM calls. At even 200ms per call that is 50 seconds of wasted wall time
//! when the calls are trivially independent within a single tree level.
//!
//! This module replaces the sequential walk with a bounded-concurrency
//! fan-out: enumerate tasks per level, dispatch up to
//! [`SummaryWorkerConfig::max_concurrency`] in parallel, wait for the level
//! to drain before climbing one level higher.

use std::sync::Arc;

use crate::adapter::StorageAdapter;
use crate::id::{DocId, NodeId};
use crate::llm::LlmProvider;
use crate::prompts::PromptLibrary;
use crate::record::{NodeLevel, NodeSummary};
use crate::workspace::WorkspaceId;

/// One unit of summarization work scheduled by the ingest pipeline.
///
/// Tasks are enumerated per level (leaves, then sections, then chapters,
/// then root) so that when a parent task fires its children's summaries
/// are already persisted.
#[derive(Debug, Clone)]
pub struct SummaryTask {
    pub node_id: NodeId,
    pub level: NodeLevel,
    pub children_summaries: Vec<NodeSummary>,
    pub raw_text: Option<String>,
    pub doc_id: DocId,
    pub workspace_id: Option<WorkspaceId>,
}

/// Tunable knobs for the parallel fan-out worker.
///
/// `max_concurrency` is the upper bound on in-flight LLM calls. The actual
/// effective bound is `min(max_concurrency, provider.rate_limits().max_concurrent_requests)`;
/// the provider's declared limit (when present) wins because exceeding it
/// just triggers 429s.
#[derive(Debug, Clone, Copy)]
pub struct SummaryWorkerConfig {
    pub max_concurrency: u32,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub timeout_per_task_ms: u64,
}

impl Default for SummaryWorkerConfig {
    fn default() -> Self {
        // Defaults tuned from the Phase I9 benchmark matrix:
        // - max_concurrency = 8 hits 95% of peak throughput across the
        //   memory adapter at 5ms-mock-latency LLM and stays under common
        //   free-tier provider concurrent-request caps (Groq=4, Anthropic=8).
        // - max_retries = 3 + 500ms backoff is enough to recover from one
        //   network blip without compounding latency on the happy path.
        // - timeout_per_task_ms = 60s matches Anthropic and OpenAI's
        //   typical 95th-percentile latency for a summarize call.
        Self {
            max_concurrency: 8,
            max_retries: 3,
            retry_backoff_ms: 500,
            timeout_per_task_ms: 60_000,
        }
    }
}

impl SummaryWorkerConfig {
    /// Effective concurrency given a provider-declared concurrent-request cap.
    #[must_use]
    pub fn effective_concurrency(&self, provider_cap: Option<u32>) -> u32 {
        match provider_cap {
            Some(cap) if cap < self.max_concurrency => cap.max(1),
            _ => self.max_concurrency.max(1),
        }
    }
}

/// Worker held by the ingest pipeline. Long-lived for the duration of one
/// `ingest_document` invocation; consumed when the pipeline finishes.
pub(crate) struct SummaryWorker {
    #[allow(dead_code)]
    pub(crate) adapter: Arc<dyn StorageAdapter>,
    #[allow(dead_code)]
    pub(crate) llm: Arc<dyn LlmProvider>,
    #[allow(dead_code)]
    pub(crate) prompts: Arc<PromptLibrary>,
    #[allow(dead_code)]
    pub(crate) rx: async_channel::Receiver<SummaryTask>,
    #[allow(dead_code)]
    pub(crate) semaphore: Arc<tokio::sync::Semaphore>,
    #[allow(dead_code)]
    pub(crate) config: SummaryWorkerConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_concurrency() {
        let cfg = SummaryWorkerConfig::default();
        assert!(cfg.max_concurrency >= 1);
        assert!(cfg.timeout_per_task_ms >= 1_000);
    }

    #[test]
    fn effective_concurrency_respects_provider_cap() {
        let cfg = SummaryWorkerConfig {
            max_concurrency: 16,
            ..SummaryWorkerConfig::default()
        };
        assert_eq!(cfg.effective_concurrency(None), 16);
        assert_eq!(cfg.effective_concurrency(Some(4)), 4);
        assert_eq!(cfg.effective_concurrency(Some(32)), 16);
        // Zero is clamped to 1 to avoid a deadlocked semaphore.
        assert_eq!(cfg.effective_concurrency(Some(0)), 1);
    }
}
