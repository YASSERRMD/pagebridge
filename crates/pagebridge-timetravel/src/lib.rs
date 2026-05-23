//! Time-travel queries: answer a question against a past corpus state.
//!
//! Pagebridge's audit log already records every node mutation. Building
//! on that, this crate provides:
//!
//! 1. A periodic full-snapshot policy: every `cadence_events` audit
//!    events, capture a `CorpusSnapshot` (Phase 40). Snapshots are
//!    persisted in a `SnapshotStore`.
//! 2. Backward-replay reconstruction: walk the audit log from a stored
//!    snapshot forward (or backward from "now") to recover the corpus
//!    state at the requested timestamp, into an in-memory overlay.
//! 3. `ask_at(t)` / `snapshot_at(t)`: facade-level APIs that materialise
//!    the overlay and run the requested operation against it.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod error;
pub mod overlay;
pub mod policy;
pub mod store;

pub use error::{TimeTravelError, Result};
pub use overlay::Overlay;
pub use policy::SnapshotPolicy;
pub use store::{FileSnapshotStore, SnapshotStore};
