//! SloMonitor: rolling time-series of request outcomes per workspace.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::burn::{BurnRate, BurnWindow};
use crate::config::SloConfig;
use crate::outcome::{HaltSignal, RequestOutcome};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloStatus {
    pub window_count: u32,
    pub p99_latency_ms_estimate: u32,
    pub error_rate: f32,
    pub spend_micro_usd: u64,
    pub fast_burn: BurnRate,
    pub slow_burn: BurnRate,
    pub healthy: bool,
}

struct Bucket {
    at: Instant,
    o: RequestOutcome,
}

pub struct SloMonitor {
    config: SloConfig,
    window: Duration,
    history: Mutex<VecDeque<Bucket>>,
}

impl SloMonitor {
    #[must_use]
    pub fn new(config: SloConfig) -> Self {
        Self {
            config,
            window: Duration::from_secs(24 * 3600),
            history: Mutex::new(VecDeque::new()),
        }
    }

    pub fn record(&self, outcome: RequestOutcome) {
        let mut h = self.history.lock();
        let now = Instant::now();
        h.push_back(Bucket {
            at: now,
            o: outcome,
        });
        while let Some(front) = h.front() {
            if now.duration_since(front.at) > self.window {
                h.pop_front();
            } else {
                break;
            }
        }
    }

    #[must_use]
    pub fn status(&self) -> SloStatus {
        let g = self.history.lock();
        let total = g.len() as u32;
        if total == 0 {
            return SloStatus {
                window_count: 0,
                p99_latency_ms_estimate: 0,
                error_rate: 0.0,
                spend_micro_usd: 0,
                fast_burn: BurnRate {
                    window: BurnWindow::Fast1h,
                    burn_multiple: 0.0,
                    error_count: 0,
                    total_count: 0,
                },
                slow_burn: BurnRate {
                    window: BurnWindow::Slow24h,
                    burn_multiple: 0.0,
                    error_count: 0,
                    total_count: 0,
                },
                healthy: true,
            };
        }
        let errors = g.iter().filter(|b| b.o.error).count() as u32;
        let error_rate = errors as f32 / total as f32;
        let mut latencies: Vec<u32> = g.iter().map(|b| b.o.latency_ms).collect();
        latencies.sort_unstable();
        let p99_idx = ((latencies.len() as f32) * 0.99) as usize;
        let p99 = *latencies
            .get(p99_idx.min(latencies.len() - 1))
            .unwrap_or(&0);
        let spend: u64 = g.iter().map(|b| b.o.cost_micro_usd).sum();
        let fast_burn = self.compute_burn(&g, Duration::from_secs(3600), BurnWindow::Fast1h);
        let slow_burn = self.compute_burn(&g, Duration::from_secs(24 * 3600), BurnWindow::Slow24h);
        let healthy = error_rate <= self.config.error_rate_max && p99 <= self.config.p99_latency_ms;
        SloStatus {
            window_count: total,
            p99_latency_ms_estimate: p99,
            error_rate,
            spend_micro_usd: spend,
            fast_burn,
            slow_burn,
            healthy,
        }
    }

    fn compute_burn(
        &self,
        history: &VecDeque<Bucket>,
        window: Duration,
        kind: BurnWindow,
    ) -> BurnRate {
        let now = Instant::now();
        let mut errors = 0u32;
        let mut total = 0u32;
        for b in history.iter().rev() {
            if now.duration_since(b.at) > window {
                break;
            }
            total += 1;
            if b.o.error {
                errors += 1;
            }
        }
        let burn_multiple = if total == 0 {
            0.0
        } else {
            (errors as f32 / total as f32) / self.config.error_rate_max.max(0.0001)
        };
        BurnRate {
            window: kind,
            burn_multiple,
            error_count: errors,
            total_count: total,
        }
    }

    /// Decide whether to halt before starting more work or while one is
    /// in flight. The caller passes the elapsed time so the monitor can
    /// project whether the budget will be exceeded.
    #[must_use]
    pub fn halt_signal(&self, elapsed_ms: u32) -> HaltSignal {
        if elapsed_ms + 200 >= self.config.p99_latency_ms {
            return HaltSignal::HaltSoft {
                reason: format!(
                    "latency budget ({}ms) about to be exceeded",
                    self.config.p99_latency_ms
                ),
            };
        }
        let status = self.status();
        if status.error_rate > self.config.error_rate_max * 2.0 {
            return HaltSignal::HaltHard {
                reason: format!(
                    "error rate {:.2}% > 2x budget {:.2}%",
                    status.error_rate * 100.0,
                    self.config.error_rate_max * 100.0
                ),
            };
        }
        HaltSignal::Proceed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_monitor_is_healthy() {
        let m = SloMonitor::new(SloConfig::default());
        assert!(m.status().healthy);
        assert_eq!(m.halt_signal(0), HaltSignal::Proceed);
    }

    #[test]
    fn halt_when_close_to_budget() {
        let cfg = SloConfig {
            p99_latency_ms: 1000,
            ..Default::default()
        };
        let m = SloMonitor::new(cfg);
        let signal = m.halt_signal(900);
        assert!(matches!(signal, HaltSignal::HaltSoft { .. }));
    }

    #[test]
    fn record_and_compute_stats() {
        let m = SloMonitor::new(SloConfig::default());
        for i in 0..10 {
            m.record(RequestOutcome {
                latency_ms: 100 * (i + 1) as u32,
                error: i == 9,
                tokens_in: 100,
                tokens_out: 100,
                cost_micro_usd: 1_000,
            });
        }
        let s = m.status();
        assert_eq!(s.window_count, 10);
        assert!(s.error_rate > 0.0);
        assert!(s.spend_micro_usd == 10_000);
    }
}
