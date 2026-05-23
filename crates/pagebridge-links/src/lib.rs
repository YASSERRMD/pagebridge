//! Cross-document soft graph for pagebridge.
//!
//! Pipeline:
//! 1. Detect references in node text (URLs, DOIs, ISBNs, section refs, title refs).
//! 2. Store them in a [`store::LinkStore`] keyed by source node.
//! 3. After each new document is ingested, run a resolution pass that walks
//!    unresolved title refs and binds them to a target document.
//!
//! v0.5.0 ships this as an in-memory store. v0.6.0 will add per-adapter
//! `pagebridge_links` persistence; the trait surface stays compatible.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::cast_precision_loss,
    clippy::redundant_clone
)]

pub mod detector;
pub mod store;

pub use detector::{detect_all, DetectedLink, LinkKind};
pub use store::{Link, LinkStore};
