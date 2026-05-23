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

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod burn;
pub mod config;
pub mod monitor;
pub mod outcome;

pub use burn::{BurnRate, BurnWindow};
pub use config::SloConfig;
pub use monitor::{SloMonitor, SloStatus};
pub use outcome::{HaltSignal, RequestOutcome};
