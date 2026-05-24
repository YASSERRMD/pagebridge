//! TenantRegistry: per-tenant limits + stats counters, exposed for
//! Prometheus scraping (the obs crate consumes this surface).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::limit::RateLimit;
use crate::limit::{LimitDecision, TenantLimits, TokenBucket};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantStats {
    pub requests_total: u64,
    pub requests_admitted: u64,
    pub requests_rejected: u64,
    pub inflight: u32,
    pub spend_micro_usd: u64,
}

pub struct TenantRegistry {
    inner: RwLock<HashMap<String, TenantSlot>>,
}

struct TenantSlot {
    limits: TenantLimits,
    request_bucket: Option<Arc<TokenBucket>>,
    inflight: u32,
    stats: TenantStats,
}

impl TenantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn upsert(&self, tenant: impl Into<String>, limits: TenantLimits) {
        let id = tenant.into();
        let request_bucket = limits
            .requests
            .as_ref()
            .map(|r| Arc::new(TokenBucket::new(r.clone())));
        let mut g = self.inner.write();
        g.insert(
            id,
            TenantSlot {
                limits,
                request_bucket,
                inflight: 0,
                stats: TenantStats::default(),
            },
        );
    }

    pub fn try_admit(&self, tenant: &str) -> LimitDecision {
        let mut g = self.inner.write();
        let Some(slot) = g.get_mut(tenant) else {
            return LimitDecision::Reject {
                reason: "unknown tenant".into(),
                retry_after_secs: 0,
            };
        };
        slot.stats.requests_total += 1;
        if let Some(cap) = slot.limits.max_inflight {
            if slot.inflight >= cap {
                slot.stats.requests_rejected += 1;
                return LimitDecision::Reject {
                    reason: format!("inflight cap {cap} reached"),
                    retry_after_secs: 1,
                };
            }
        }
        if let Some(bucket) = &slot.request_bucket {
            let d = bucket.try_take(1);
            if let LimitDecision::Reject { .. } = d {
                slot.stats.requests_rejected += 1;
                return d;
            }
        }
        slot.inflight += 1;
        slot.stats.requests_admitted += 1;
        LimitDecision::Allow
    }

    pub fn release(&self, tenant: &str, cost_micro_usd: u64) {
        let mut g = self.inner.write();
        if let Some(slot) = g.get_mut(tenant) {
            if slot.inflight > 0 {
                slot.inflight -= 1;
            }
            slot.stats.spend_micro_usd += cost_micro_usd;
            slot.stats.inflight = slot.inflight;
        }
    }

    #[must_use]
    pub fn stats(&self, tenant: &str) -> Option<TenantStats> {
        self.inner.read().get(tenant).map(|s| s.stats.clone())
    }

    #[must_use]
    pub fn snapshot(&self) -> HashMap<String, TenantStats> {
        self.inner
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.stats.clone()))
            .collect()
    }
}

impl Default for TenantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_then_rejects_when_inflight_cap_hits() {
        let r = TenantRegistry::new();
        r.upsert(
            "acme",
            TenantLimits {
                max_inflight: Some(2),
                ..Default::default()
            },
        );
        assert!(matches!(r.try_admit("acme"), LimitDecision::Allow));
        assert!(matches!(r.try_admit("acme"), LimitDecision::Allow));
        let d = r.try_admit("acme");
        assert!(matches!(d, LimitDecision::Reject { .. }));
        r.release("acme", 0);
        assert!(matches!(r.try_admit("acme"), LimitDecision::Allow));
    }

    #[test]
    fn rate_limit_enforced() {
        let r = TenantRegistry::new();
        r.upsert(
            "acme",
            TenantLimits {
                requests: Some(RateLimit {
                    capacity: 2,
                    refill_per_sec: 1,
                }),
                ..Default::default()
            },
        );
        assert!(matches!(r.try_admit("acme"), LimitDecision::Allow));
        assert!(matches!(r.try_admit("acme"), LimitDecision::Allow));
        let d = r.try_admit("acme");
        assert!(matches!(d, LimitDecision::Reject { .. }));
    }
}
