use std::sync::Arc;

use crate::source::{FederatedCandidate, FederatedSource};

#[derive(Debug, Clone)]
pub struct MergedCandidate {
    pub candidate: FederatedCandidate,
    pub z_score: f32,
}

/// Query every source in parallel, then merge by z-score over the
/// per-source candidates. Top-`final_k` are returned by descending
/// z-score.
pub async fn merge_candidates(
    sources: &[Arc<dyn FederatedSource>],
    query: &str,
    per_source_top_k: usize,
    final_k: usize,
) -> Vec<MergedCandidate> {
    let futures = sources
        .iter()
        .map(|s| {
            let s = Arc::clone(s);
            let q = query.to_owned();
            async move { s.candidates(&q, per_source_top_k).await }
        })
        .collect::<Vec<_>>();
    let per_source: Vec<Vec<FederatedCandidate>> = futures::future::join_all(futures).await;

    let mut merged: Vec<MergedCandidate> = Vec::new();
    for cands in per_source {
        if cands.is_empty() {
            continue;
        }
        let n = cands.len() as f32;
        let mean: f32 = cands.iter().map(|c| c.score).sum::<f32>() / n;
        let var: f32 = cands.iter().map(|c| (c.score - mean).powi(2)).sum::<f32>() / n;
        let std = var.sqrt().max(1e-6);
        for c in cands {
            let z = (c.score - mean) / std;
            merged.push(MergedCandidate {
                candidate: c,
                z_score: z,
            });
        }
    }
    merged.sort_by(|a, b| {
        b.z_score
            .partial_cmp(&a.z_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged.truncate(final_k);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct Scripted {
        id: String,
        results: Vec<FederatedCandidate>,
    }

    #[async_trait]
    impl FederatedSource for Scripted {
        fn source_id(&self) -> &str {
            &self.id
        }
        async fn candidates(&self, _q: &str, top_k: usize) -> Vec<FederatedCandidate> {
            self.results.iter().take(top_k).cloned().collect()
        }
    }

    fn cand(src: &str, nid: &str, score: f32) -> FederatedCandidate {
        FederatedCandidate {
            source_id: src.into(),
            node_id: nid.into(),
            score,
            title: nid.into(),
        }
    }

    #[tokio::test]
    async fn z_score_merge_orders_correctly() {
        let s1: Arc<dyn FederatedSource> = Arc::new(Scripted {
            id: "policy".into(),
            results: vec![
                cand("policy", "n1", 9.0),
                cand("policy", "n2", 5.0),
                cand("policy", "n3", 1.0),
            ],
        });
        let s2: Arc<dyn FederatedSource> = Arc::new(Scripted {
            id: "legal".into(),
            results: vec![
                cand("legal", "m1", 0.9),
                cand("legal", "m2", 0.5),
                cand("legal", "m3", 0.1),
            ],
        });
        let merged = merge_candidates(&[s1, s2], "q", 3, 4).await;
        assert_eq!(merged.len(), 4);
        // Top result must come from one of the per-source winners.
        let top_id = &merged[0].candidate.node_id;
        assert!(top_id == "n1" || top_id == "m1");
    }
}
