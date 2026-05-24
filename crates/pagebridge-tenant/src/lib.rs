//! Per-tenant resource isolation: rate limits + concurrency caps + fair queueing.

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
    clippy::suboptimal_flops,
    clippy::match_wildcard_for_single_variants,
    clippy::significant_drop_in_scrutinee,
    clippy::significant_drop_tightening
)]

pub mod limit;
pub mod queue;
pub mod registry;

pub use limit::{LimitDecision, RateLimit, TenantLimits, TokenBucket};
pub use queue::{Drr, DrrError};
pub use registry::{TenantRegistry, TenantStats};
