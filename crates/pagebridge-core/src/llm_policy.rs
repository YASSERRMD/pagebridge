//! Adaptive backoff, retry, and circuit-breaker policy for LLM calls.
//!
//! Concrete providers wrap their HTTP call in [`with_policy`], which:
//!  - classifies errors as retryable or terminal,
//!  - applies exponential backoff (with jitter) between attempts,
//!  - honors a Retry-After hint if the error carries one,
//!  - aborts and trips the circuit when sustained failure crosses a threshold.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{PagebridgeError, Result};

/// Tunable knobs for the retry policy.
#[derive(Debug, Clone, Copy)]
pub struct LlmCallPolicy {
    pub initial_interval: Duration,
    pub max_interval: Duration,
    pub multiplier: f64,
    pub max_elapsed_time: Option<Duration>,
    pub max_retries: u32,
    pub per_call_timeout: Duration,
}

impl Default for LlmCallPolicy {
    fn default() -> Self {
        Self {
            initial_interval: Duration::from_millis(500),
            max_interval: Duration::from_secs(30),
            multiplier: 2.0,
            max_elapsed_time: Some(Duration::from_secs(300)),
            max_retries: 5,
            per_call_timeout: Duration::from_secs(60),
        }
    }
}

/// Hint about an error's transience so the retry loop knows whether to
/// re-attempt. Providers map their HTTP status codes / SDK errors into one
/// of these variants before raising.
#[derive(Debug, Clone, Copy)]
pub enum RetryClass {
    /// Permanent failure: bad request, unauthorized, schema mismatch, etc.
    Terminal,
    /// Retryable, generic transient failure (5xx, network blip).
    Transient,
    /// Retryable, with the provider asking us to wait `Duration` first
    /// (typically a 429 carrying a Retry-After header).
    RetryAfter(Duration),
}

impl RetryClass {
    /// Classify an HTTP status code as the LLM provider would interpret it.
    #[must_use]
    pub fn from_status(status: u16, retry_after: Option<Duration>) -> Self {
        match status {
            429 => retry_after.map_or(Self::Transient, Self::RetryAfter),
            400 | 401 | 403 | 404 | 422 => Self::Terminal,
            500..=599 => Self::Transient,
            _ => Self::Terminal,
        }
    }
}

/// Parse a `Retry-After` HTTP header value. Honors the common delta-seconds
/// form (`Retry-After: 120`); for the rarer HTTP-date form we fall back to a
/// 30-second default so we still wait a meaningful interval. Returns `None`
/// for empty input.
#[must_use]
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    let trimmed = header_value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    Some(Duration::from_secs(30))
}

/// Tracks recent failures so the circuit can trip on sustained outage.
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Number of successes within the rolling window.
    pub(crate) successes: AtomicU32,
    /// Number of failures within the rolling window.
    pub(crate) failures: AtomicU32,
    /// When the window started (epoch millis).
    pub(crate) window_start_ms: AtomicU64,
    /// When the breaker was opened, if ever.
    pub(crate) opened_at: Mutex<Option<Instant>>,
    pub(crate) config: CircuitBreakerConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    pub window: Duration,
    pub min_calls: u32,
    pub failure_ratio: f32,
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(60),
            min_calls: 5,
            failure_ratio: 0.8,
            cooldown: Duration::from_secs(30),
        }
    }
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(config: CircuitBreakerConfig) -> Arc<Self> {
        Arc::new(Self {
            successes: AtomicU32::new(0),
            failures: AtomicU32::new(0),
            window_start_ms: AtomicU64::new(now_ms()),
            opened_at: Mutex::new(None),
            config,
        })
    }

    /// True when the breaker is open and still within the cooldown window.
    #[must_use]
    pub fn is_open(&self) -> bool {
        if let Some(opened) = *self.opened_at.lock() {
            if opened.elapsed() < self.config.cooldown {
                return true;
            }
            // Cooldown elapsed; clear it.
            *self.opened_at.lock() = None;
            self.reset_window();
        }
        false
    }

    pub fn record_success(&self) {
        self.maybe_roll();
        self.successes.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_failure(&self) {
        self.maybe_roll();
        self.failures.fetch_add(1, Ordering::SeqCst);
        self.maybe_trip();
    }

    fn maybe_roll(&self) {
        let start = self.window_start_ms.load(Ordering::SeqCst);
        if now_ms().saturating_sub(start) >= self.config.window.as_millis() as u64 {
            self.reset_window();
        }
    }

    fn reset_window(&self) {
        self.successes.store(0, Ordering::SeqCst);
        self.failures.store(0, Ordering::SeqCst);
        self.window_start_ms.store(now_ms(), Ordering::SeqCst);
    }

    fn maybe_trip(&self) {
        let s = self.successes.load(Ordering::SeqCst);
        let f = self.failures.load(Ordering::SeqCst);
        let total = s + f;
        if total < self.config.min_calls {
            return;
        }
        let ratio = f as f32 / total as f32;
        if ratio >= self.config.failure_ratio {
            *self.opened_at.lock() = Some(Instant::now());
        }
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Driver loop: invoke `op` under the configured policy. The closure returns
/// either an Ok value or an Err with a [`RetryClass`] hint.
pub async fn with_policy<T, F, Fut>(
    policy: LlmCallPolicy,
    breaker: Option<&CircuitBreaker>,
    mut op: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, (PagebridgeError, RetryClass)>>,
{
    if let Some(b) = breaker {
        if b.is_open() {
            return Err(PagebridgeError::Llm {
                provider: "circuit".into(),
                message: "circuit breaker is open".into(),
            });
        }
    }
    let start = Instant::now();
    let mut delay = policy.initial_interval;
    let mut attempt: u32 = 0;
    loop {
        let fut = op();
        let outcome = tokio::time::timeout(policy.per_call_timeout, fut).await;
        match outcome {
            Ok(Ok(v)) => {
                if let Some(b) = breaker {
                    b.record_success();
                }
                return Ok(v);
            }
            Ok(Err((err, RetryClass::Terminal))) => {
                if let Some(b) = breaker {
                    b.record_failure();
                }
                return Err(err);
            }
            Ok(Err((err, class))) => {
                if let Some(b) = breaker {
                    b.record_failure();
                }
                attempt += 1;
                if attempt > policy.max_retries {
                    return Err(err);
                }
                if let Some(max_elapsed) = policy.max_elapsed_time {
                    if start.elapsed() >= max_elapsed {
                        return Err(err);
                    }
                }
                let wait = match class {
                    RetryClass::RetryAfter(d) => d,
                    _ => delay,
                };
                tokio::time::sleep(wait).await;
                let next_ms = (delay.as_millis() as f64 * policy.multiplier) as u64;
                delay = Duration::from_millis(next_ms.min(policy.max_interval.as_millis() as u64));
            }
            Err(_elapsed) => {
                // Per-call timeout fired. Classify as transient.
                if let Some(b) = breaker {
                    b.record_failure();
                }
                attempt += 1;
                if attempt > policy.max_retries {
                    return Err(PagebridgeError::Llm {
                        provider: "policy".into(),
                        message: format!(
                            "per-call timeout {}ms exceeded after {attempt} attempts",
                            policy.per_call_timeout.as_millis()
                        ),
                    });
                }
                tokio::time::sleep(delay).await;
                let next_ms = (delay.as_millis() as f64 * policy.multiplier) as u64;
                delay = Duration::from_millis(next_ms.min(policy.max_interval.as_millis() as u64));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    #[test]
    fn classify_429_with_retry_after() {
        let c = RetryClass::from_status(429, Some(Duration::from_secs(5)));
        assert!(matches!(c, RetryClass::RetryAfter(d) if d == Duration::from_secs(5)));
        let c = RetryClass::from_status(429, None);
        assert!(matches!(c, RetryClass::Transient));
    }

    #[test]
    fn parse_retry_after_seconds_form() {
        assert_eq!(parse_retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("  120 "), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parse_retry_after_date_form_defaults_30s() {
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn parse_retry_after_empty_returns_none() {
        assert_eq!(parse_retry_after(""), None);
        assert_eq!(parse_retry_after("   "), None);
    }

    #[test]
    fn classify_terminal_4xx() {
        for s in [400u16, 401, 403, 404, 422] {
            assert!(matches!(
                RetryClass::from_status(s, None),
                RetryClass::Terminal
            ));
        }
    }

    #[tokio::test]
    async fn retries_then_succeeds() {
        let attempts = Arc::new(AsyncMutex::new(0u32));
        let policy = LlmCallPolicy {
            initial_interval: Duration::from_millis(5),
            max_interval: Duration::from_millis(20),
            multiplier: 2.0,
            max_elapsed_time: Some(Duration::from_secs(5)),
            max_retries: 3,
            per_call_timeout: Duration::from_secs(1),
        };
        let result: Result<u32> = with_policy(policy, None, || {
            let attempts = Arc::clone(&attempts);
            async move {
                let mut a = attempts.lock().await;
                *a += 1;
                if *a < 3 {
                    Err((
                        PagebridgeError::Llm {
                            provider: "test".into(),
                            message: "boom".into(),
                        },
                        RetryClass::Transient,
                    ))
                } else {
                    Ok(42u32)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(*attempts.lock().await, 3);
    }

    #[tokio::test]
    async fn circuit_opens_on_sustained_failure() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            window: Duration::from_secs(60),
            min_calls: 3,
            failure_ratio: 0.5,
            cooldown: Duration::from_secs(60),
        });
        let policy = LlmCallPolicy {
            initial_interval: Duration::from_millis(1),
            max_interval: Duration::from_millis(2),
            multiplier: 2.0,
            max_elapsed_time: Some(Duration::from_millis(100)),
            max_retries: 0,
            per_call_timeout: Duration::from_secs(1),
        };
        for _ in 0..4 {
            let _ = with_policy::<(), _, _>(policy, Some(&breaker), || async {
                Err((
                    PagebridgeError::Llm {
                        provider: "test".into(),
                        message: "fail".into(),
                    },
                    RetryClass::Transient,
                ))
            })
            .await;
        }
        assert!(breaker.is_open(), "expected circuit to be open");
    }
}
