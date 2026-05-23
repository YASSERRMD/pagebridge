//! Evaluation framework for pagebridge.
//!
//! Built around three types: [`EvalSet`] (corpus + question/ground-truth list),
//! [`EvalResult`] (per-question metrics), and the [`run`] function that ties
//! them together using a `Pagebridge` instance. Outputs are CSV-friendly so
//! results compose with the CLI's existing pipeline.
//!
//! Computed metrics per question:
//! - `retrieval_recall_at_k`: did the navigator surface the ground-truth
//!   leaves in the top-k citations?
//! - `citation_precision`: of citations returned, what fraction match ground
//!   truth?
//! - `bleu_lite`: a tiny BLEU-style n-gram overlap score (no stemming, no
//!   brevity penalty); useful as a fast proxy in CI. Wire a real BLEU
//!   implementation when comparing across runs.
//! - `latency_ms`: end-to-end wall time of `ask`.
//! - `tokens_in / tokens_out`: from the query trace.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_long_first_doc_paragraph,
    clippy::unnecessary_to_owned
)]

pub mod metrics;
pub mod runner;
pub mod schema;

pub use metrics::{bleu_lite, citation_precision, retrieval_recall_at_k};
pub use runner::run;
pub use schema::{EvalQuestion, EvalResult, EvalSet, EvalSummary};
