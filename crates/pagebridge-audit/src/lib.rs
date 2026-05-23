//! pagebridge-audit: tamper-evident audit logging for cognitive retrieval.
//!
//! Every public-API boundary in pagebridge emits one or more [`AuditEvent`]s
//! through an [`AuditWriter`]. Events are:
//!
//! 1. Hash-chained (`prev_hash` -> `event_hash`) so any in-place mutation
//!    breaks the chain at the modified row.
//! 2. Ed25519 signed so an adversary with write access to the storage
//!    cannot forge events without the signing key.
//! 3. Periodically Merkle-anchored (every N events) and published to an
//!    integrity store. Verifying the anchors offline proves the log is
//!    intact between snapshots.
//!
//! The crate compiles without any optional features. HTTP-based sinks
//! (Splunk HEC, Elastic, OpenSearch, S3 object lock) are gated behind
//! `http-sinks`.
//!
//! ## Compliance hooks
//!
//! The schema captures the fields required by HIPAA §164.312(b), GDPR
//! Article 30, SOX ITGC, and the EU AI Act high-risk system requirements.
//! See `docs/COMPLIANCE.md` for the mapping.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_panics_doc
)]

pub mod error;
pub mod event;
pub mod merkle;
pub mod sign;
pub mod sinks;
pub mod transparency;
pub mod verifier;
pub mod writer;

pub use error::{AuditError, Result};
pub use event::{
    AuditAction, AuditEvent, AuditOutcome, AuditResource, PolicyDecision, Principal,
};
pub use merkle::{
    merkle_proof, merkle_root, verify_inclusion, InclusionProof, MerkleBatch, ProofNode,
};
pub use sign::{
    canonical_event_hash, seal_event, verify_event, SignatureVerifier, SigningSecret,
};
pub use transparency::{NoopTransparencyClient, TransparencyClient, TrillianEntry};
pub use verifier::{replay_chain, verify_batch, ReplayReport};
pub use writer::{AuditSink, AuditWriter, WriterConfig};
