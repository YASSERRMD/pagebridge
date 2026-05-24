//! Hybrid edge/cloud routing.
//!
//! Pagebridge runs every query against the local configuration first;
//! a [`ConfidenceEstimator`] scores the local answer, and an
//! [`EscalationPolicy`] decides whether to escalate to a (more
//! expensive, more capable) cloud configuration.
//!
//! Privacy mode is the default: on escalation, only the question and
//! the most-relevant local snippet are sent to the cloud. The full
//! corpus stays on-device.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::map_unwrap_or,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::manual_clamp,
    clippy::needless_pass_by_value,
    clippy::trivially_copy_pass_by_ref,
    clippy::similar_names,
    clippy::redundant_clone,
    clippy::useless_vec,
    clippy::default_trait_access,
    clippy::single_match_else,
    clippy::match_same_arms,
    clippy::needless_collect,
    clippy::unnecessary_wraps,
    clippy::redundant_closure_for_method_calls,
    clippy::iter_on_single_items,
    clippy::option_if_let_else,
    clippy::elidable_lifetime_names,
    clippy::suboptimal_flops,
    clippy::match_wildcard_for_single_variants,
    clippy::significant_drop_in_scrutinee,
    clippy::significant_drop_tightening
)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceSignals {
    pub top_bm25_score: f32,
    pub nav_step_count: u32,
    pub synthesis_logprob: Option<f32>,
    pub groundedness_self_check: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct ConfidenceEstimator;

impl ConfidenceEstimator {
    /// Returns a confidence in [0.0, 1.0]. Simple linear model the
    /// platform can tune from telemetry; defaults are conservative.
    #[must_use]
    pub fn score(&self, s: &ConfidenceSignals) -> f32 {
        let bm25 = (s.top_bm25_score / 10.0).min(1.0).max(0.0);
        let nav = if s.nav_step_count <= 3 { 1.0 } else { 0.6 };
        let logp = s
            .synthesis_logprob
            .map(|l| (l / -5.0 + 1.0).clamp(0.0, 1.0))
            .unwrap_or(0.7);
        let ground = s.groundedness_self_check.unwrap_or(0.7);
        (0.3 * bm25 + 0.2 * nav + 0.2 * logp + 0.3 * ground).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPolicy {
    pub confidence_threshold: f32,
    pub latency_budget_remaining_ms_for_escalation: u32,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.4,
            latency_budget_remaining_ms_for_escalation: 2000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    StayLocal,
    EscalateToCloud { reason: String },
}

impl EscalationPolicy {
    #[must_use]
    pub fn decide(&self, confidence: f32, latency_remaining_ms: u32) -> Decision {
        if confidence < self.confidence_threshold
            && latency_remaining_ms >= self.latency_budget_remaining_ms_for_escalation
        {
            return Decision::EscalateToCloud {
                reason: format!(
                    "local confidence {confidence:.2} < {:.2}",
                    self.confidence_threshold
                ),
            };
        }
        Decision::StayLocal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeMetrics {
    pub total: u64,
    pub stayed_local: u64,
    pub escalated: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_confidence_stays_local() {
        let p = EscalationPolicy::default();
        assert_eq!(p.decide(0.95, 5000), Decision::StayLocal);
    }

    #[test]
    fn low_confidence_with_budget_escalates() {
        let p = EscalationPolicy::default();
        match p.decide(0.1, 5000) {
            Decision::EscalateToCloud { .. } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn no_budget_keeps_local_even_low_confidence() {
        let p = EscalationPolicy::default();
        assert_eq!(p.decide(0.1, 500), Decision::StayLocal);
    }

    #[test]
    fn confidence_score_in_range() {
        let e = ConfidenceEstimator;
        let c = e.score(&ConfidenceSignals {
            top_bm25_score: 8.0,
            nav_step_count: 2,
            synthesis_logprob: Some(-0.5),
            groundedness_self_check: Some(0.9),
        });
        assert!(c > 0.5);
    }
}
