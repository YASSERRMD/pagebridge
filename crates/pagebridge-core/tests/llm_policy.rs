//! Integration tests for the LLM retry / backoff / circuit-breaker policy.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::needless_pass_by_value,
    clippy::elidable_lifetime_names,
    clippy::manual_let_else,
    clippy::if_not_else,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_borrows_for_generic_args,
    clippy::uninlined_format_args,
    clippy::semicolon_if_nothing_returned,
    clippy::needless_lifetimes,
    clippy::useless_vec,
    clippy::map_unwrap_or
)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pagebridge_core::error::PagebridgeError;
use pagebridge_core::llm_policy::with_policy;
use pagebridge_core::{
    parse_retry_after, CircuitBreaker, CircuitBreakerConfig, LlmCallPolicy, RetryClass,
};

#[tokio::test]
async fn retry_after_hint_is_honored() {
    // Inject a Retry-After of 80ms on the first two attempts, then succeed.
    let attempts = Arc::new(AtomicU32::new(0));
    let policy = LlmCallPolicy {
        initial_interval: Duration::from_millis(1),
        max_interval: Duration::from_millis(1),
        multiplier: 1.0,
        max_elapsed_time: Some(Duration::from_secs(5)),
        max_retries: 5,
        per_call_timeout: Duration::from_secs(1),
    };
    let t0 = Instant::now();
    let result: Result<u32, _> = with_policy(policy, None, || {
        let attempts = Arc::clone(&attempts);
        async move {
            let n = attempts.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err((
                    PagebridgeError::Llm {
                        provider: "test".into(),
                        message: "429".into(),
                    },
                    RetryClass::RetryAfter(Duration::from_millis(80)),
                ))
            } else {
                Ok(7u32)
            }
        }
    })
    .await;
    let elapsed = t0.elapsed();
    assert_eq!(result.unwrap(), 7);
    // Two 80ms waits = at least 160ms total, well above the 2ms backoff
    // baseline.
    assert!(
        elapsed >= Duration::from_millis(150),
        "expected Retry-After to throttle to >=150ms, observed {elapsed:?}"
    );
}

#[tokio::test]
async fn circuit_short_circuits_after_trip() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        window: Duration::from_secs(60),
        min_calls: 2,
        failure_ratio: 0.5,
        cooldown: Duration::from_secs(60),
    });
    let policy = LlmCallPolicy {
        initial_interval: Duration::from_millis(1),
        max_interval: Duration::from_millis(1),
        multiplier: 1.0,
        max_elapsed_time: Some(Duration::from_millis(100)),
        max_retries: 0,
        per_call_timeout: Duration::from_secs(1),
    };
    for _ in 0..3 {
        let _ = with_policy::<(), _, _>(policy, Some(&breaker), || async {
            Err((
                PagebridgeError::Llm {
                    provider: "test".into(),
                    message: "boom".into(),
                },
                RetryClass::Transient,
            ))
        })
        .await;
    }
    assert!(breaker.is_open(), "circuit should be open");
    // Subsequent calls return the circuit-open error without invoking op.
    let invoked = Arc::new(AtomicU32::new(0));
    let invoked2 = Arc::clone(&invoked);
    let err = with_policy::<u32, _, _>(policy, Some(&breaker), || {
        let invoked2 = Arc::clone(&invoked2);
        async move {
            invoked2.fetch_add(1, Ordering::SeqCst);
            Ok(42)
        }
    })
    .await
    .unwrap_err();
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
    assert!(matches!(err, PagebridgeError::Llm { .. }));
}

#[tokio::test]
async fn per_call_timeout_aborts_slow_op() {
    let policy = LlmCallPolicy {
        initial_interval: Duration::from_millis(1),
        max_interval: Duration::from_millis(1),
        multiplier: 1.0,
        max_elapsed_time: Some(Duration::from_millis(500)),
        max_retries: 0,
        per_call_timeout: Duration::from_millis(20),
    };
    let result: Result<u32, _> = with_policy(policy, None, || async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(1u32)
    })
    .await;
    assert!(result.is_err(), "per-call timeout must abort slow op");
}

#[test]
fn parse_retry_after_returns_none_on_empty() {
    assert_eq!(parse_retry_after(""), None);
}
