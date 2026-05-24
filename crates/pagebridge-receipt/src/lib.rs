//! Verifiable Answer Receipts.
//!
//! Every `Pagebridge::ask` can ship an `AnswerReceipt` attached to its
//! `Answer`. The receipt records exactly which node versions were used,
//! exactly which LLM was called (with its sampling parameters), exactly
//! which prompt template versions were applied, and a cryptographic
//! Merkle root over the corpus snapshot.
//!
//! The receipt is signed with the same Ed25519 key the audit log uses
//! (re-export the `SigningSecret` from `pagebridge-audit`) so a single
//! key chain verifies both the events and the answers.
//!
//! Canonical encoding: SSZ-compatible (deterministic field ordering,
//! length-prefixed byte sequences). We do not yet pull the `ssz_rs`
//! crate; we ship a hand-written canonical JSON encoder whose output is
//! identical across pagebridge versions.
//!
//! See `docs/spec/verifiable-receipts-v1.md` for the wire spec.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::too_long_first_doc_paragraph,
    clippy::useless_conversion,
    clippy::default_trait_access,
    clippy::stable_sort_primitive
)]

pub mod error;
pub mod facade_issuer;
pub mod fingerprint;
pub mod receipt;
pub mod verify;

pub use error::{ReceiptError, Result};
pub use facade_issuer::FacadeReceiptIssuer;
pub use fingerprint::{LlmFingerprint, NodeReference, PromptVersionMap};
pub use receipt::{issue_receipt, AnswerReceipt, ReceiptInputs};
pub use verify::{verify_receipt, ReceiptVerifier};
