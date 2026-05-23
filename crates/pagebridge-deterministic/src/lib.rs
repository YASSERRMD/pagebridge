//! Deterministic retrieval for pagebridge.
//!
//! In deterministic mode every layer of the pipeline produces the same
//! output for the same inputs:
//!
//! - The LLM provider passes a configurable `seed` plus `T=0` / `top_p=1`.
//! - Storage adapters produce a canonical ordering for every query.
//! - The corpus state is pinned to a content-addressed snapshot id.
//!
//! Same question + same corpus snapshot + same `DeterministicMode` config
//! = byte-identical answer, today or in three years.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod adapter_canonical;
pub mod config;
pub mod error;
pub mod llm_contract;
pub mod snapshot;

pub use adapter_canonical::{order_by_for, tiebreaker_for};
pub use config::{DeterministicMode, QueryOrder};
pub use error::{DeterministicError, Result};
pub use llm_contract::DeterminismContract;
pub use snapshot::{compute_snapshot_id, CorpusSnapshot, SnapshotEntry};
