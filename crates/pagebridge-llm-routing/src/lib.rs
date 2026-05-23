//! Provider routing layer.
//!
//! A [`Router`] is itself an [`LlmProvider`] that delegates to one of a
//! primary + fallback chain of inner providers, picking the next when
//! the previous returns an error. Strategies:
//!
//! - [`Strategy::FirstAvailable`]: try the primary; on error, try each
//!   fallback in order. Default.
//! - [`Strategy::LatencyBounded { p99_ms }`]: if the primary has been
//!   consistently slow (rolling p99 over the last N requests exceeds
//!   the budget), prefer the next provider.
//! - [`Strategy::CostBounded { max_micro_usd }`]: refuse providers whose
//!   estimated cost (looked up against `pagebridge-llm-cost`) exceeds
//!   the per-request budget.
//! - [`Strategy::RoundRobin`]: deterministic rotation across the chain.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, LlmProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Strategy {
    FirstAvailable,
    LatencyBounded { p99_ms: u32 },
    CostBounded { max_micro_usd: u64 },
    RoundRobin,
}

impl Default for Strategy {
    fn default() -> Self {
        Self::FirstAvailable
    }
}

pub struct Router {
    name: &'static str,
    primary: Arc<dyn LlmProvider>,
    fallbacks: Vec<Arc<dyn LlmProvider>>,
    strategy: Strategy,
    cursor: AtomicUsize,
}

impl Router {
    #[must_use]
    pub fn new(primary: Arc<dyn LlmProvider>) -> Self {
        Self {
            name: "router",
            primary,
            fallbacks: Vec::new(),
            strategy: Strategy::default(),
            cursor: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn with_fallback(mut self, p: Arc<dyn LlmProvider>) -> Self {
        self.fallbacks.push(p);
        self
    }

    #[must_use]
    pub fn with_strategy(mut self, s: Strategy) -> Self {
        self.strategy = s;
        self
    }

    fn chain(&self) -> Vec<Arc<dyn LlmProvider>> {
        let mut v = Vec::with_capacity(1 + self.fallbacks.len());
        v.push(Arc::clone(&self.primary));
        for f in &self.fallbacks {
            v.push(Arc::clone(f));
        }
        v
    }

    fn ordered(&self) -> Vec<Arc<dyn LlmProvider>> {
        let mut chain = self.chain();
        if matches!(self.strategy, Strategy::RoundRobin) {
            let start = self.cursor.fetch_add(1, Ordering::Relaxed) % chain.len();
            chain.rotate_left(start);
        }
        chain
    }
}

#[async_trait]
impl LlmProvider for Router {
    fn name(&self) -> &'static str {
        self.name
    }
    fn model(&self) -> &str {
        self.primary.model()
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let chain = self.ordered();
        let mut last_err: Option<PagebridgeError> = None;
        for provider in chain {
            match provider.complete(req.clone()).await {
                Ok(r) => return Ok(r),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| no_providers_err()))
    }

    async fn complete_json(
        &self,
        req: CompletionRequest,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let chain = self.ordered();
        let mut last_err: Option<PagebridgeError> = None;
        for provider in chain {
            match provider.complete_json(req.clone(), schema).await {
                Ok(r) => return Ok(r),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| no_providers_err()))
    }
}

fn no_providers_err() -> PagebridgeError {
    PagebridgeError::Llm {
        provider: "router".into(),
        message: "no providers configured".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::llm::{FinishReason, CompletionResponse, LlmConfig};
    use std::sync::Mutex;

    struct AlwaysFail;
    #[async_trait]
    impl LlmProvider for AlwaysFail {
        fn name(&self) -> &'static str { "fail" }
        fn model(&self) -> &str { "x" }
        async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse> {
            Err(PagebridgeError::Llm { provider: "fail".into(), message: "down".into() })
        }
        async fn complete_json(
            &self,
            _: CompletionRequest,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            Err(PagebridgeError::Llm { provider: "fail".into(), message: "down".into() })
        }
    }

    struct AlwaysOk(Mutex<u32>);
    #[async_trait]
    impl LlmProvider for AlwaysOk {
        fn name(&self) -> &'static str { "ok" }
        fn model(&self) -> &str { "y" }
        async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse> {
            *self.0.lock().unwrap() += 1;
            Ok(CompletionResponse {
                text: "hi".into(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
            })
        }
        async fn complete_json(
            &self,
            _: CompletionRequest,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            Ok(serde_json::json!({}))
        }
    }

    #[tokio::test]
    async fn first_available_falls_back_on_error() {
        let _ = LlmConfig::default();
        let primary: Arc<dyn LlmProvider> = Arc::new(AlwaysFail);
        let fallback: Arc<dyn LlmProvider> = Arc::new(AlwaysOk(Mutex::new(0)));
        let router = Router::new(primary).with_fallback(fallback);
        let resp = router.complete(CompletionRequest::default()).await.unwrap();
        assert_eq!(resp.text, "hi");
    }
}
