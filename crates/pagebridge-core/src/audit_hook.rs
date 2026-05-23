//! Audit hook trait. Implemented by `pagebridge-audit::FacadeAuditHook`.
//!
//! The facade emits one of these on every public-API boundary. The hook
//! sees only what pagebridge-core can describe; the full
//! [`pagebridge-audit::AuditEvent`] is constructed inside the audit crate
//! to keep this crate dependency-free of signing/Merkle code.

use std::sync::Arc;

use crate::id::DocId;
use crate::workspace::WorkspaceId;

#[derive(Debug, Clone)]
pub struct AskAuditFields {
    pub workspace_id: WorkspaceId,
    pub adapter: String,
    pub question_hash: String,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u32,
    pub success: bool,
    pub failure_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IngestAuditFields {
    pub workspace_id: WorkspaceId,
    pub adapter: String,
    pub doc_id: DocId,
    pub byte_count: u64,
    pub leaf_count: u32,
    pub latency_ms: u32,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct DeleteAuditFields {
    pub workspace_id: WorkspaceId,
    pub adapter: String,
    pub doc_id: DocId,
    pub success: bool,
}

#[async_trait::async_trait]
pub trait AuditHook: Send + Sync + 'static {
    async fn on_ask(&self, fields: AskAuditFields);
    async fn on_ingest(&self, fields: IngestAuditFields);
    async fn on_delete(&self, fields: DeleteAuditFields);
}

/// No-op hook used when no audit subsystem is configured. The facade
/// still calls into the hook to keep the call-site shape uniform.
pub struct NoopAuditHook;

#[async_trait::async_trait]
impl AuditHook for NoopAuditHook {
    async fn on_ask(&self, _: AskAuditFields) {}
    async fn on_ingest(&self, _: IngestAuditFields) {}
    async fn on_delete(&self, _: DeleteAuditFields) {}
}

#[must_use]
pub fn noop() -> Arc<dyn AuditHook> {
    Arc::new(NoopAuditHook)
}
