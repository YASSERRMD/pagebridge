use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityConfig {
    pub sample_rate: f32,
    pub drift_delta: f32,
    pub rolling_window_days: u32,
    pub baseline_window_days: u32,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            sample_rate: 0.05,
            drift_delta: 0.05,
            rolling_window_days: 7,
            baseline_window_days: 30,
        }
    }
}
