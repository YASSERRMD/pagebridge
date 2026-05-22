//! pagebridge-core: cognitive retrieval primitives.
//!
//! This crate hosts the core types, the `StorageAdapter` trait, the `LlmProvider`
//! trait, the prompt library, the ingest pipeline, and the navigation/synthesis
//! engine. Higher-level crates (adapters, LLM providers, the umbrella crate)
//! depend on this one.

pub mod error;
pub mod id;
pub mod record;

pub use error::{PagebridgeError, Result};
pub use id::{DocId, NodeId};
pub use record::{NodeLevel, NodeRecord, NodeSummary};

/// Crate version string, used by the CLI and by trace metadata.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
