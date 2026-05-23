//! Multi-window burn-rate computation. Industry convention:
//!
//! - Fast burn: 1-hour window, alert at >14.4x budget consumption rate.
//! - Slow burn: 24-hour window, alert at >1x budget consumption rate.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BurnWindow {
    Fast1h,
    Slow24h,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRate {
    pub window: BurnWindow,
    pub burn_multiple: f32,
    pub error_count: u32,
    pub total_count: u32,
}

impl BurnRate {
    #[must_use]
    pub fn alert_threshold(&self) -> f32 {
        match self.window {
            BurnWindow::Fast1h => 14.4,
            BurnWindow::Slow24h => 1.0,
        }
    }

    #[must_use]
    pub fn should_alert(&self) -> bool {
        self.burn_multiple > self.alert_threshold()
    }
}
