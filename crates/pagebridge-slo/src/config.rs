use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloConfig {
    pub p99_latency_ms: u32,
    pub error_rate_max: f32,
    pub monthly_cost_usd_max: f32,
    pub tokens_per_question_max: u32,
}

impl Default for SloConfig {
    fn default() -> Self {
        Self {
            p99_latency_ms: 3000,
            error_rate_max: 0.01,
            monthly_cost_usd_max: 500.0,
            tokens_per_question_max: 4000,
        }
    }
}
