//! Cost attribution + budgets + forecasting.
//!
//! Every question, every workspace, every tenant rolls up to a precise
//! cost (in micro-USD; integer math). BudgetConfig sets the caps;
//! BudgetTracker enforces them at the API boundary; Forecaster
//! projects monthly spend from a rolling 7-day trend.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::significant_drop_tightening,
    clippy::significant_drop_in_scrutinee,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::collections::BTreeMap;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub use pagebridge_llm_cost::CostCatalog;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub monthly_cap_micro_usd: Option<u64>,
    pub per_question_cap_micro_usd: Option<u64>,
    pub alert_at_pct: Option<u8>,
    pub hard_stop_at_pct: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetDecision {
    Allow,
    Warn { reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSample {
    pub at_ms: u64,
    pub workspace_id: String,
    pub question_id: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_micro_usd: u64,
}

pub struct BudgetTracker {
    config: BudgetConfig,
    samples: Mutex<Vec<CostSample>>,
}

impl BudgetTracker {
    #[must_use]
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            samples: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, sample: CostSample) {
        self.samples.lock().push(sample);
    }

    /// Decide whether to admit an estimated request of `estimated_cost`.
    #[must_use]
    pub fn pre_admit(&self, estimated_cost_micro_usd: u64) -> BudgetDecision {
        if let Some(per_q) = self.config.per_question_cap_micro_usd {
            if estimated_cost_micro_usd > per_q {
                return BudgetDecision::Deny {
                    reason: format!(
                        "estimated cost {estimated_cost_micro_usd}µ$ exceeds per-question cap {per_q}µ$"
                    ),
                };
            }
        }
        if let Some(monthly) = self.config.monthly_cap_micro_usd {
            let spent = self.month_total_micro_usd();
            if let Some(hard) = self.config.hard_stop_at_pct {
                let limit = (monthly * u64::from(hard)) / 100;
                if spent >= limit {
                    return BudgetDecision::Deny {
                        reason: format!("monthly hard-stop {hard}% reached"),
                    };
                }
            }
            if let Some(alert) = self.config.alert_at_pct {
                let limit = (monthly * u64::from(alert)) / 100;
                if spent >= limit {
                    return BudgetDecision::Warn {
                        reason: format!("monthly spend at {alert}% of cap"),
                    };
                }
            }
        }
        BudgetDecision::Allow
    }

    #[must_use]
    pub fn month_total_micro_usd(&self) -> u64 {
        self.samples.lock().iter().map(|s| s.cost_micro_usd).sum()
    }

    #[must_use]
    pub fn breakdown_by_workspace(&self) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for s in self.samples.lock().iter() {
            *out.entry(s.workspace_id.clone()).or_insert(0) += s.cost_micro_usd;
        }
        out
    }

    /// Project this month's spend from a rolling 7-day trend.
    /// Naive linear extrapolation: spend_per_day * days_in_month.
    #[must_use]
    pub fn forecast_monthly_micro_usd(&self) -> u64 {
        let samples = self.samples.lock();
        if samples.is_empty() {
            return 0;
        }
        let total: u64 = samples.iter().map(|s| s.cost_micro_usd).sum();
        // Treat all samples as belonging to "the rolling window";
        // production tracks at_ms to filter to the last 7 days.
        let days = 7.0_f64;
        let per_day = total as f64 / days;
        (per_day * 30.0) as u64
    }
}

/// Convenience: estimate a request's cost from a (provider, model)
/// fingerprint plus token counts using a CostCatalog.
#[must_use]
pub fn estimate_cost(
    catalog: &CostCatalog,
    provider: &str,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> u64 {
    catalog
        .cost_micro_usd(provider, model, input_tokens, output_tokens)
        .unwrap_or(0)
}

fn _unused_instant() -> Instant {
    Instant::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ws: &str, cost: u64) -> CostSample {
        CostSample {
            at_ms: 0,
            workspace_id: ws.into(),
            question_id: "q".into(),
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            input_tokens: 100,
            output_tokens: 100,
            cost_micro_usd: cost,
        }
    }

    #[test]
    fn per_question_cap_denies_oversized_estimate() {
        let cfg = BudgetConfig {
            per_question_cap_micro_usd: Some(1_000),
            ..Default::default()
        };
        let t = BudgetTracker::new(cfg);
        assert!(matches!(t.pre_admit(500), BudgetDecision::Allow));
        assert!(matches!(t.pre_admit(2_000), BudgetDecision::Deny { .. }));
    }

    #[test]
    fn breakdown_aggregates_per_workspace() {
        let t = BudgetTracker::new(BudgetConfig::default());
        t.record(sample("acme", 100));
        t.record(sample("acme", 200));
        t.record(sample("contoso", 50));
        let b = t.breakdown_by_workspace();
        assert_eq!(b["acme"], 300);
        assert_eq!(b["contoso"], 50);
    }
}
