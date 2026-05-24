//! Sensitivity labels + access-control policy.
//!
//! Every node can carry a [`SensitivityLabel`]. The retrieval engine
//! evaluates the caller's [`AccessPolicy`] against the candidate node
//! before letting it into the navigation set or the synthesis context.
//! Default policy is least-privilege: tokens must explicitly grant
//! access to anything beyond `Public`.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::iter_on_single_items,
    clippy::needless_collect,
    clippy::too_long_first_doc_paragraph,
    clippy::match_same_arms,
    clippy::redundant_clone,
    clippy::needless_pass_by_value,
    clippy::default_trait_access,
    clippy::useless_vec,
    clippy::unnecessary_wraps,
    clippy::single_match_else,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

pub mod classifier;
pub mod label;
pub mod policy;

pub use classifier::{LabelDetector, RegexDetector};
pub use label::SensitivityLabel;
pub use policy::{AccessPolicy, AllowSet};
