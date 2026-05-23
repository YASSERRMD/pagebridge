use serde::{Deserialize, Serialize};

use crate::scorer::ScoreSample;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    pub baseline_p50: f32,
    pub rolling_p50: f32,
    pub delta: f32,
    pub drifted: bool,
}

pub struct DriftDetector {
    pub delta_threshold: f32,
}

impl DriftDetector {
    #[must_use]
    pub fn new(delta_threshold: f32) -> Self {
        Self { delta_threshold }
    }

    /// Compare the rolling window of faithfulness scores against the
    /// baseline. Returns drifted=true if rolling p50 dropped more than
    /// `delta_threshold` below baseline.
    #[must_use]
    pub fn evaluate_faithfulness(
        &self,
        baseline: &[ScoreSample],
        rolling: &[ScoreSample],
    ) -> DriftReport {
        let baseline_p50 = p50_faithfulness(baseline);
        let rolling_p50 = p50_faithfulness(rolling);
        let delta = baseline_p50 - rolling_p50;
        let drifted = delta > self.delta_threshold;
        DriftReport {
            baseline_p50,
            rolling_p50,
            delta,
            drifted,
        }
    }
}

fn p50_faithfulness(samples: &[ScoreSample]) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    let mut values: Vec<f32> = samples.iter().map(|s| s.scores.faithfulness).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    values[mid]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::ScoreTriple;

    fn sample(faith: f32) -> ScoreSample {
        ScoreSample {
            ts_ns: 0,
            workspace_id: "acme".into(),
            query_id: "q".into(),
            scores: ScoreTriple {
                faithfulness: faith,
                citation_accuracy: 0.8,
                answer_relevance: 0.8,
            },
        }
    }

    #[test]
    fn drift_detected_when_rolling_drops() {
        let baseline = vec![sample(0.9), sample(0.92), sample(0.88)];
        let rolling = vec![sample(0.7), sample(0.6), sample(0.65)];
        let d = DriftDetector::new(0.05);
        let r = d.evaluate_faithfulness(&baseline, &rolling);
        assert!(r.drifted);
    }

    #[test]
    fn no_drift_when_within_threshold() {
        let baseline = vec![sample(0.9)];
        let rolling = vec![sample(0.88)];
        let d = DriftDetector::new(0.05);
        let r = d.evaluate_faithfulness(&baseline, &rolling);
        assert!(!r.drifted);
    }
}
