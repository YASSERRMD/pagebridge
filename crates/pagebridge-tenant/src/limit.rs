//! Token-bucket rate limits and per-tenant caps.

use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub capacity: u32,
    pub refill_per_sec: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenantLimits {
    pub requests: Option<RateLimit>,
    pub input_tokens_per_min: Option<RateLimit>,
    pub output_tokens_per_min: Option<RateLimit>,
    pub max_inflight: Option<u32>,
    pub daily_cost_usd_cap: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitDecision {
    Allow,
    Reject {
        reason: String,
        retry_after_secs: u32,
    },
}

pub struct TokenBucket {
    config: RateLimit,
    inner: Mutex<Inner>,
}

struct Inner {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    #[must_use]
    pub fn new(config: RateLimit) -> Self {
        Self {
            config: config.clone(),
            inner: Mutex::new(Inner {
                tokens: f64::from(config.capacity),
                last_refill: Instant::now(),
            }),
        }
    }

    pub fn try_take(&self, n: u32) -> LimitDecision {
        let mut g = self.inner.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(g.last_refill).as_secs_f64();
        let refilled = elapsed * f64::from(self.config.refill_per_sec);
        g.tokens = (g.tokens + refilled).min(f64::from(self.config.capacity));
        g.last_refill = now;
        if g.tokens >= f64::from(n) {
            g.tokens -= f64::from(n);
            LimitDecision::Allow
        } else {
            let deficit = f64::from(n) - g.tokens;
            let secs = (deficit / f64::from(self.config.refill_per_sec.max(1))).ceil() as u32;
            LimitDecision::Reject {
                reason: format!("rate limit: short by {deficit:.1} tokens"),
                retry_after_secs: secs.max(1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_admits_until_empty() {
        let b = TokenBucket::new(RateLimit {
            capacity: 3,
            refill_per_sec: 1,
        });
        for _ in 0..3 {
            assert_eq!(b.try_take(1), LimitDecision::Allow);
        }
        let d = b.try_take(1);
        assert!(matches!(d, LimitDecision::Reject { .. }));
    }
}
