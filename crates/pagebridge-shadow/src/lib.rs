//! Shadow traffic: route a sample of production queries through an
//! alternate configuration, compare outcomes, and report whether the
//! candidate is ready to promote.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowConfig {
    pub enabled: bool,
    pub sample_rate: f32,
    pub candidate_label: String,
    pub baseline_label: String,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_rate: 0.05,
            candidate_label: "candidate".into(),
            baseline_label: "baseline".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowSample {
    pub at_ms: u64,
    pub query_id: String,
    pub baseline_score: f32,
    pub candidate_score: f32,
    pub baseline_latency_ms: u32,
    pub candidate_latency_ms: u32,
    pub baseline_cost_micro_usd: u64,
    pub candidate_cost_micro_usd: u64,
}

pub struct ShadowReport {
    pub sample_count: usize,
    pub mean_score_delta: f32,
    pub mean_latency_delta_ms: f32,
    pub mean_cost_delta_micro_usd: f64,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Promote,
    Hold,
    Revert,
}

pub struct ShadowAggregator {
    samples: Mutex<Vec<ShadowSample>>,
}

impl ShadowAggregator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            samples: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, sample: ShadowSample) {
        self.samples.lock().push(sample);
    }

    #[must_use]
    pub fn report(&self) -> ShadowReport {
        let g = self.samples.lock();
        if g.is_empty() {
            return ShadowReport {
                sample_count: 0,
                mean_score_delta: 0.0,
                mean_latency_delta_ms: 0.0,
                mean_cost_delta_micro_usd: 0.0,
                verdict: Verdict::Hold,
            };
        }
        let n = g.len() as f32;
        let mean_score: f32 = g
            .iter()
            .map(|s| s.candidate_score - s.baseline_score)
            .sum::<f32>()
            / n;
        let mean_lat: f32 = g
            .iter()
            .map(|s| s.candidate_latency_ms as f32 - s.baseline_latency_ms as f32)
            .sum::<f32>()
            / n;
        let mean_cost: f64 = g
            .iter()
            .map(|s| s.candidate_cost_micro_usd as f64 - s.baseline_cost_micro_usd as f64)
            .sum::<f64>()
            / f64::from(n);
        let verdict = if mean_score > 0.02 && mean_lat < 100.0 && mean_cost < 100.0 {
            Verdict::Promote
        } else if mean_score < -0.05 {
            Verdict::Revert
        } else {
            Verdict::Hold
        };
        ShadowReport {
            sample_count: g.len(),
            mean_score_delta: mean_score,
            mean_latency_delta_ms: mean_lat,
            mean_cost_delta_micro_usd: mean_cost,
            verdict,
        }
    }
}

impl Default for ShadowAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(score_delta: f32) -> ShadowSample {
        ShadowSample {
            at_ms: 0,
            query_id: "q".into(),
            baseline_score: 0.8,
            candidate_score: 0.8 + score_delta,
            baseline_latency_ms: 100,
            candidate_latency_ms: 120,
            baseline_cost_micro_usd: 1_000,
            candidate_cost_micro_usd: 1_050,
        }
    }

    #[test]
    fn promote_when_candidate_better_and_cheap() {
        let a = ShadowAggregator::new();
        for _ in 0..10 {
            a.record(sample(0.05));
        }
        let r = a.report();
        assert_eq!(r.verdict, Verdict::Promote);
    }

    #[test]
    fn revert_when_candidate_worse() {
        let a = ShadowAggregator::new();
        for _ in 0..10 {
            a.record(sample(-0.10));
        }
        let r = a.report();
        assert_eq!(r.verdict, Verdict::Revert);
    }
}
