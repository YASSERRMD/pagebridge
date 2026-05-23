//! Federated retrieval: a single ask spans multiple pagebridge sources.
//!
//! Each `FederatedSource` produces its own ranked candidates. The
//! federation merges them by z-score normalization (so different
//! scoring scales become comparable), then hands the merged set to a
//! single navigation pass. Citations are tagged with their source.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod merge;
pub mod source;

pub use merge::{merge_candidates, MergedCandidate};
pub use source::{FederatedCandidate, FederatedSource};
