//! Rate-limit aware dispatch helpers for the parallel summary fan-out.
//!
//! Reads [`crate::llm::RateLimits`] from the provider and constructs a
//! governor token bucket plus an effective concurrency cap so the fan-out
//! never trips provider-side throttling.

use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};

use crate::llm::RateLimits;

/// Quota-based RPM limiter built atop the `governor` crate.
pub type RpmLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Bundle the scheduler needs to dispatch LLM calls: an optional RPM limiter,
/// an optional TPM limiter, and the effective concurrency cap.
pub struct DispatchScheduler {
    pub rpm: Option<Arc<RpmLimiter>>,
    pub tpm: Option<Arc<RpmLimiter>>,
    pub effective_concurrency: u32,
    /// Consecutive 429-class hits observed by the fan-out. Used to halve the
    /// effective dispatch rate when a provider starts throttling so the
    /// retry loop does not produce a thundering herd.
    pub recent_429s: std::sync::atomic::AtomicU32,
}

impl DispatchScheduler {
    /// Build a scheduler from a provider's declared limits and a caller-side
    /// `max_concurrency` ceiling. Returned scheduler's `effective_concurrency`
    /// is `min(max_concurrency, provider.max_concurrent_requests)`, clamped to
    /// at least 1.
    #[must_use]
    pub fn from_limits(limits: RateLimits, max_concurrency: u32) -> Self {
        let effective = match limits.max_concurrent_requests {
            Some(c) if c < max_concurrency => c.max(1),
            _ => max_concurrency.max(1),
        };
        let rpm = limits
            .requests_per_minute
            .and_then(NonZeroU32::new)
            .map(|n| Arc::new(RateLimiter::direct(Quota::per_minute(n))));
        let tpm = limits
            .tokens_per_minute
            .and_then(NonZeroU32::new)
            .map(|n| Arc::new(RateLimiter::direct(Quota::per_minute(n))));
        Self {
            rpm,
            tpm,
            effective_concurrency: effective,
            recent_429s: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Record a 429 event. Used by the adaptive-throttle policy so repeated
    /// hits within a short window can halve the effective concurrency for a
    /// cooldown period.
    pub fn note_throttle(&self) {
        self.recent_429s
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Reset the throttle counter (called periodically by the fan-out).
    pub fn clear_throttle(&self) {
        self.recent_429s
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }

    /// True when more than `threshold` consecutive 429-class errors have been
    /// observed since the last reset.
    #[must_use]
    pub fn is_storming(&self, threshold: u32) -> bool {
        self.recent_429s.load(std::sync::atomic::Ordering::SeqCst) >= threshold
    }

    /// Await one RPM permit, if a limiter is configured. Returns immediately
    /// when there is no limit declared. Tokens-per-minute is similar but the
    /// caller knows the per-call token estimate, so it is exposed separately.
    pub async fn await_request_permit(&self) {
        if let Some(rl) = &self.rpm {
            rl.until_ready().await;
        }
    }

    /// Await `tokens` worth of TPM permits. No-op if no TPM limit declared.
    pub async fn await_token_permits(&self, tokens: u32) {
        if let Some(rl) = &self.tpm {
            let n = NonZeroU32::new(tokens.max(1)).expect("max(1) is non-zero");
            // governor's until_n_ready returns Err only when n exceeds the
            // quota burst; in that case we fall back to until_ready which
            // waits the natural delay for the bucket to drain.
            if rl.until_n_ready(n).await.is_err() {
                rl.until_ready().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_creates_no_limiters() {
        let s = DispatchScheduler::from_limits(RateLimits::unlimited(), 8);
        assert!(s.rpm.is_none());
        assert!(s.tpm.is_none());
        assert_eq!(s.effective_concurrency, 8);
    }

    #[test]
    fn provider_cap_clamps_concurrency() {
        let s = DispatchScheduler::from_limits(RateLimits::groq_free(), 16);
        assert_eq!(s.effective_concurrency, 4);
        assert!(s.rpm.is_some());
    }

    #[test]
    fn local_only_caps_concurrency() {
        let s = DispatchScheduler::from_limits(RateLimits::local(), 8);
        assert!(s.rpm.is_none());
        assert!(s.tpm.is_none());
        assert_eq!(s.effective_concurrency, 2);
    }
}
