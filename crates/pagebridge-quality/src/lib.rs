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

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod config;
pub mod judge;
pub mod scorer;
pub mod store;
pub mod drift;

pub use config::QualityConfig;
pub use judge::{Judge, NoopJudge, ScoreTriple};
pub use scorer::{ScoreSample, Scorer};
pub use store::{MemoryQualityStore, QualityStore};
pub use drift::{DriftDetector, DriftReport};
