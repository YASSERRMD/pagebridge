//! Sensitivity labels + access-control policy.
//!
//! Every node can carry a [`SensitivityLabel`]. The retrieval engine
//! evaluates the caller's [`AccessPolicy`] against the candidate node
//! before letting it into the navigation set or the synthesis context.
//! Default policy is least-privilege: tokens must explicitly grant
//! access to anything beyond `Public`.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod classifier;
pub mod label;
pub mod policy;

pub use classifier::{LabelDetector, RegexDetector};
pub use label::SensitivityLabel;
pub use policy::{AccessPolicy, AllowSet};
