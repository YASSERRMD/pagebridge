//! Canonical audit event schema.
//!
//! Every retrieval, every node read, every LLM call produces one of these.
//! Events are hash-chained (`prev_hash` -> `event_hash`) and Ed25519 signed
//! so any tampering breaks the chain at the modified event.
//!
//! Schema stability: the on-disk encoding is canonical JSON (sorted keys,
//! no whitespace). Future extensions add fields with `Option<T>` and never
//! reorder or rename existing fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::workspace::WorkspaceId;

/// The kind of action a single audit event records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Ingest,
    AskStart,
    NavigateStep,
    NodeRead,
    Bm25Query,
    LlmCall,
    SynthesisChunk,
    AskComplete,
    Update,
    Delete,
    Export,
    Admin,
}

impl AuditAction {
    /// Stable lowercase tag, used in storage and metrics labels.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ingest => "ingest",
            Self::AskStart => "ask_start",
            Self::NavigateStep => "navigate_step",
            Self::NodeRead => "node_read",
            Self::Bm25Query => "bm25_query",
            Self::LlmCall => "llm_call",
            Self::SynthesisChunk => "synthesis_chunk",
            Self::AskComplete => "ask_complete",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Export => "export",
            Self::Admin => "admin",
        }
    }
}

/// Who initiated this action. Resolved from the Biscuit token (Phase 28)
/// if a token was presented; otherwise carries the anonymous principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Stable identifier (often a token's subject claim, or "anonymous").
    pub id: String,
    /// Display-only label captured at the time of the event.
    pub label: Option<String>,
    /// Token fingerprint (hex of sha256(token bytes)). Anonymous calls leave this empty.
    pub token_fingerprint: Option<String>,
}

impl Principal {
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            id: "anonymous".to_string(),
            label: None,
            token_fingerprint: None,
        }
    }
}

/// What resource an event touched. Keep variants small and `Copy`-friendly
/// where possible so the schema stays cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "resource_kind", rename_all = "snake_case")]
pub enum AuditResource {
    Workspace,
    Document { doc_id: DocId },
    Node { node_id: NodeId },
    Query { question_hash: String },
    Adapter { name: String },
    Other { kind: String, value: String },
}

/// Outcome of the action. Denials and errors carry a short reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Denied { reason: String },
    Error { kind: String },
    Halted,
}

/// Policy decisions taken during the event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Which policy versions were applied (name -> version).
    pub applied: BTreeMap<String, u32>,
    /// Was the resource allowed?
    pub allowed: bool,
    /// Optional short justification (free-form).
    pub justification: Option<String>,
}

impl PolicyDecision {
    #[must_use]
    pub fn allowed() -> Self {
        Self {
            applied: BTreeMap::new(),
            allowed: true,
            justification: None,
        }
    }
}

/// One audit event. The `event_hash` and `signature` are populated by the
/// log writer (`AuditWriter`) when the event is appended; constructing an
/// event yourself leaves them zeroed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Monotonic per-workspace event identifier (ULID is sortable and
    /// embeds the timestamp).
    pub event_id: Ulid,
    pub timestamp_ns: u128,
    pub workspace_id: WorkspaceId,
    pub principal: Principal,
    pub action: AuditAction,
    pub resource: AuditResource,
    pub outcome: AuditOutcome,
    pub adapter: String,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u32,
    pub sensitivity_label: Option<String>,
    pub policy_decision: PolicyDecision,
    pub parent_event: Option<Ulid>,
    /// Hash of the prior event in this workspace's chain. Genesis = all zeros.
    #[serde(with = "hex_bytes")]
    pub prev_hash: [u8; 32],
    /// sha256 of the canonical JSON of this event with `event_hash` and
    /// `signature` zeroed.
    #[serde(with = "hex_bytes")]
    pub event_hash: [u8; 32],
    /// Ed25519 signature over `event_hash`.
    #[serde(with = "hex_bytes_vec")]
    pub signature: Vec<u8>,
    pub key_id: String,
}

impl AuditEvent {
    /// Build a new unsigned, unhashed event. The writer will populate
    /// `event_hash`, `signature`, and `key_id`.
    #[must_use]
    pub fn unsigned(
        workspace_id: WorkspaceId,
        principal: Principal,
        action: AuditAction,
        resource: AuditResource,
        outcome: AuditOutcome,
        adapter: impl Into<String>,
    ) -> Self {
        Self {
            event_id: Ulid::new(),
            timestamp_ns: now_ns(),
            workspace_id,
            principal,
            action,
            resource,
            outcome,
            adapter: adapter.into(),
            llm_provider: None,
            llm_model: None,
            input_tokens: 0,
            output_tokens: 0,
            latency_ms: 0,
            sensitivity_label: None,
            policy_decision: PolicyDecision::default(),
            parent_event: None,
            prev_hash: [0u8; 32],
            event_hash: [0u8; 32],
            signature: Vec::new(),
            key_id: String::new(),
        }
    }
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if v.len() != 32 {
            return Err(serde::de::Error::custom("expected 32-byte hex"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }
}

mod hex_bytes_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_str_is_stable() {
        assert_eq!(AuditAction::AskStart.as_str(), "ask_start");
        assert_eq!(AuditAction::SynthesisChunk.as_str(), "synthesis_chunk");
    }

    #[test]
    fn event_json_roundtrip() {
        let ws = WorkspaceId::new("acme").unwrap();
        let e = AuditEvent::unsigned(
            ws,
            Principal::anonymous(),
            AuditAction::AskStart,
            AuditResource::Query {
                question_hash: "abcd".into(),
            },
            AuditOutcome::Success,
            "embedded",
        );
        let s = serde_json::to_string(&e).unwrap();
        let back: AuditEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back.action, AuditAction::AskStart);
    }
}
