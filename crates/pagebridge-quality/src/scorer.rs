use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::QualityConfig;
use crate::judge::{Judge, ScoreTriple};
use crate::store::QualityStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreSample {
    pub ts_ns: u128,
    pub workspace_id: String,
    pub query_id: String,
    pub scores: ScoreTriple,
}

pub struct Scorer {
    pub judge: Arc<dyn Judge>,
    pub store: Arc<dyn QualityStore>,
    pub config: QualityConfig,
}

impl Scorer {
    pub fn new(judge: Arc<dyn Judge>, store: Arc<dyn QualityStore>, config: QualityConfig) -> Self {
        Self {
            judge,
            store,
            config,
        }
    }

    /// Decide whether to sample this query, and if so, score it and
    /// persist the sample. `coin` should be a uniform u32 in [0, u32::MAX].
    pub async fn maybe_score(
        &self,
        coin: u32,
        workspace_id: &str,
        query_id: &str,
        question: &str,
        answer: &str,
        cited_excerpts: &[String],
    ) {
        let rate = self.config.sample_rate.clamp(0.0, 1.0);
        if rate <= 0.0 {
            return;
        }
        let threshold = (rate * (u32::MAX as f32)) as u32;
        if coin > threshold {
            return;
        }
        let scores = self.judge.score(question, answer, cited_excerpts).await;
        let sample = ScoreSample {
            ts_ns: now_ns(),
            workspace_id: workspace_id.to_string(),
            query_id: query_id.to_string(),
            scores,
        };
        self.store.append(sample).await;
    }
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::NoopJudge;
    use crate::store::MemoryQualityStore;

    #[tokio::test]
    async fn always_samples_when_rate_is_one() {
        let cfg = QualityConfig {
            sample_rate: 1.0,
            ..Default::default()
        };
        let store = Arc::new(MemoryQualityStore::new());
        let s = Scorer::new(Arc::new(NoopJudge), store.clone(), cfg);
        s.maybe_score(0, "acme", "q1", "q", "a", &["e".into()])
            .await;
        assert_eq!(store.since(0).await.len(), 1);
    }

    #[tokio::test]
    async fn never_samples_when_rate_is_zero() {
        let cfg = QualityConfig {
            sample_rate: 0.0,
            ..Default::default()
        };
        let store = Arc::new(MemoryQualityStore::new());
        let s = Scorer::new(Arc::new(NoopJudge), store.clone(), cfg);
        s.maybe_score(0, "acme", "q1", "q", "a", &["e".into()])
            .await;
        assert_eq!(store.since(0).await.len(), 0);
    }
}
