//! Per-tenant resource isolation: rate limits + concurrency caps + fair queueing.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod limit;
pub mod queue;
pub mod registry;

pub use limit::{LimitDecision, RateLimit, TenantLimits, TokenBucket};
pub use queue::{Drr, DrrError};
pub use registry::{TenantRegistry, TenantStats};
