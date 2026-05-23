//! Read-replica coordination primitives.
//!
//! For multi-process deployments where many pagebridge readers share one
//! backing database, in-memory caches in the readers drift out of date when
//! the writer changes nodes. This module defines:
//!
//! - [`ReplicationRole`]: writer or reader.
//! - [`ReplicationConfig`]: how often readers poll the invalidation log.
//! - [`InvalidationKind`] + [`InvalidationEvent`]: the wire shape of an
//!   invalidation entry. Adapters that opt into replication append these to
//!   a `pagebridge_invalidation` table on writes; readers poll the same
//!   table and dispatch cache invalidations.
//!
//! v1.0.0 ships the type surface and the in-process channel; per-adapter
//! `pagebridge_invalidation` table migrations land alongside the workspace
//! migration in v0.4.

use serde::{Deserialize, Serialize};

use crate::id::{DocId, NodeId};

/// Process role for replication-aware deployments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplicationRole {
    /// May upsert nodes, ingest documents, delete documents.
    Writer,
    /// Read-only. Polls the invalidation log to keep caches fresh.
    Reader,
}

impl ReplicationRole {
    /// Borrow the wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Writer => "writer",
            Self::Reader => "reader",
        }
    }
}

/// Configuration for a replication-aware pagebridge instance.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReplicationConfig {
    pub role: ReplicationRole,
    /// How often readers poll the invalidation log (seconds). Ignored by
    /// writers. Default: 2 seconds.
    pub invalidation_poll_secs: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            role: ReplicationRole::Writer,
            invalidation_poll_secs: 2,
        }
    }
}

/// Why a cache entry should be invalidated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationKind {
    UpsertNode,
    DeleteDoc,
    LinkResolved,
    SummaryCacheBust,
}

impl InvalidationKind {
    /// Borrow the wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UpsertNode => "upsert_node",
            Self::DeleteDoc => "delete_doc",
            Self::LinkResolved => "link_resolved",
            Self::SummaryCacheBust => "summary_cache_bust",
        }
    }
}

/// One invalidation entry. A writer appends one of these on every change;
/// readers consume them in sequence order and refresh their caches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidationEvent {
    pub sequence: u64,
    pub workspace_id: String,
    pub kind: InvalidationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<DocId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    pub emitted_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_as_lowercase() {
        let role = ReplicationRole::Writer;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"writer\"");
    }

    #[test]
    fn config_default_is_writer_two_second_poll() {
        let cfg = ReplicationConfig::default();
        assert_eq!(cfg.role, ReplicationRole::Writer);
        assert_eq!(cfg.invalidation_poll_secs, 2);
    }

    #[test]
    fn invalidation_kind_round_trips_to_snake_case() {
        let kind = InvalidationKind::UpsertNode;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"upsert_node\"");
        let back: InvalidationKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
}
