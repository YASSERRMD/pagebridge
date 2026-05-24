//! Continuous groundedness monitoring.
//!
//! Pagebridge samples a configurable fraction of production answers
//! (default 5%) and scores each on three dimensions:
//!
//! 1. Faithfulness: are claims in the answer supported by cited leaves?
//! 2. Citation accuracy: are cited leaves actually relevant?
//! 3. Answer relevance: does the answer address the question?
//!
//! Scores are persisted as time-series; drift is detected when the
//! 7-day rolling p50 drops more than `delta` below the 30-day baseline.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::manual_clamp
)]

pub mod config;
pub mod drift;
pub mod judge;
pub mod scorer;
pub mod store;

pub use config::QualityConfig;
pub use drift::{DriftDetector, DriftReport};
pub use judge::{Judge, NoopJudge, ScoreTriple};
pub use scorer::{ScoreSample, Scorer};
pub use store::{MemoryQualityStore, QualityStore};
