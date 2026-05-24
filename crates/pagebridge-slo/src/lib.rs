//! SLO-driven operations for pagebridge.
//!
//! Each workspace can carry an [`SloConfig`] with p99 latency budget,
//! error rate cap, monthly cost cap, and per-question token cap. The
//! runtime ([`SloMonitor`]) accumulates per-request outcomes into a
//! rolling time-series and produces:
//!
//! - [`SloStatus`] snapshots (current burn rate, time to exhaust).
//! - [`HaltSignal`]s when the budget is about to be exceeded.
//! - Burn-rate alarms compatible with Prometheus / OTLP (multi-window:
//!   1-hour fast burn, 24-hour slow burn).

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::map_unwrap_or,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::manual_clamp,
    clippy::needless_pass_by_value,
    clippy::trivially_copy_pass_by_ref,
    clippy::similar_names,
    clippy::redundant_clone,
    clippy::useless_vec,
    clippy::default_trait_access,
    clippy::single_match_else,
    clippy::match_same_arms,
    clippy::needless_collect,
    clippy::unnecessary_wraps,
    clippy::redundant_closure_for_method_calls,
    clippy::iter_on_single_items,
    clippy::option_if_let_else,
    clippy::elidable_lifetime_names,
    clippy::unnecessary_literal_bound,
    clippy::significant_drop_tightening,
    clippy::significant_drop_in_scrutinee
)]

pub mod burn;
pub mod config;
pub mod monitor;
pub mod outcome;

pub use burn::{BurnRate, BurnWindow};
pub use config::SloConfig;
pub use monitor::{SloMonitor, SloStatus};
pub use outcome::{HaltSignal, RequestOutcome};
