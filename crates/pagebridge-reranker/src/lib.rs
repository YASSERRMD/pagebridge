//! Reranker trait + provider implementations.
//!
//! Rerankers take a query plus a list of candidate documents and
//! return them re-ordered by relevance, with new scores. Pagebridge
//! uses them at the navigation step to re-score BM25 candidates before
//! handing them to the navigation LLM.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod stub;
#[cfg(feature = "voyage")]
pub mod voyage;
#[cfg(feature = "cohere")]
pub mod cohere;

#[derive(Debug, Error)]
pub enum RerankerError {
    #[error("provider {provider}: {message}")]
    Provider { provider: String, message: String },
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankedDoc {
    pub index: usize,
    pub score: f32,
}

pub type Result<T> = std::result::Result<T, RerankerError>;

#[async_trait]
pub trait Reranker: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn model(&self) -> &str;
    async fn rerank(
        &self,
        query: &str,
        docs: &[String],
        top_k: usize,
    ) -> Result<Vec<RerankedDoc>>;
}
