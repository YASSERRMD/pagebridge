use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScoreTriple {
    pub faithfulness: f32,
    pub citation_accuracy: f32,
    pub answer_relevance: f32,
}

#[async_trait]
pub trait Judge: Send + Sync + 'static {
    async fn score(
        &self,
        question: &str,
        answer: &str,
        cited_excerpts: &[String],
    ) -> ScoreTriple;
}

/// Test/dev judge that returns a deterministic mid-range score so the
/// pipeline can be exercised without a real LLM.
pub struct NoopJudge;

#[async_trait]
impl Judge for NoopJudge {
    async fn score(
        &self,
        _question: &str,
        answer: &str,
        excerpts: &[String],
    ) -> ScoreTriple {
        let answer_len = answer.len() as f32;
        let excerpts_len: f32 = excerpts.iter().map(|s| s.len() as f32).sum();
        // Synthetic but stable: longer cited content -> higher faithfulness.
        let f = (excerpts_len / (answer_len + 1.0)).min(1.0).max(0.5);
        ScoreTriple {
            faithfulness: f,
            citation_accuracy: 0.8,
            answer_relevance: if answer.trim().is_empty() { 0.0 } else { 0.85 },
        }
    }
}
