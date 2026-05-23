//! Audit sinks: append-only destinations for sealed events and Merkle batches.
//!
//! Each sink implements [`crate::AuditSink`]. The default sink is the
//! [`file::FileSink`] which appends NDJSON lines to a rotating directory.
//! Production deployments typically chain a local sink (for fast tailing)
//! with an external WORM sink (for compliance retention).

pub mod file;
pub mod tee;
pub mod worm;

#[cfg(feature = "http-sinks")]
pub mod elastic;
#[cfg(feature = "http-sinks")]
pub mod splunk;

pub use file::FileSink;
pub use tee::TeeSink;
pub use worm::WormFileSink;

#[cfg(feature = "http-sinks")]
pub use elastic::ElasticSink;
#[cfg(feature = "http-sinks")]
pub use splunk::SplunkHecSink;
