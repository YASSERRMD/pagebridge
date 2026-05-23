//! Deterministic stub reranker: returns docs in their original order
//! with monotonically decreasing synthetic scores. Useful for tests and
//! as a fallback when no real reranker is configured.

use async_trait::async_trait;

use crate::{RerankedDoc, Reranker, Result};

pub struct StubReranker;

#[async_trait]
impl Reranker for StubReranker {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn model(&self) -> &str {
        "noop"
    }
    async fn rerank(
        &self,
        _query: &str,
        docs: &[String],
        top_k: usize,
    ) -> Result<Vec<RerankedDoc>> {
        let n = top_k.min(docs.len());
        Ok((0..n)
            .map(|i| RerankedDoc {
                index: i,
                #[allow(clippy::cast_precision_loss)]
                score: 1.0_f32 - (i as f32) / (n.max(1) as f32),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_top_k_in_original_order() {
        let docs: Vec<String> = (0..10).map(|i| format!("d{i}")).collect();
        let r = StubReranker;
        let out = r.rerank("q", &docs, 5).await.unwrap();
        assert_eq!(out.len(), 5);
        assert!(out[0].score > out[4].score);
    }
}
