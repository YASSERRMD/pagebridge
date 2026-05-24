//! `FacadeAuditHook`: bridges the core `AuditHook` trait to an `AuditWriter`.
//!
//! Construct one of these and pass it to `PagebridgeOptions::with_audit_hook`.
//! Every public-API call on the facade becomes a sealed audit event in
//! every configured sink.

use std::sync::Arc;

use pagebridge_core::audit_hook::{
    AskAuditFields, AuditHook, DeleteAuditFields, IngestAuditFields,
};

use crate::event::{AuditAction, AuditEvent, AuditOutcome, AuditResource, Principal};
use crate::writer::AuditWriter;

/// Hook that funnels every facade boundary into an [`AuditWriter`].
pub struct FacadeAuditHook {
    writer: Arc<AuditWriter>,
    principal: Principal,
}

impl FacadeAuditHook {
    #[must_use]
    pub fn new(writer: Arc<AuditWriter>) -> Self {
        Self {
            writer,
            principal: Principal::anonymous(),
        }
    }

    #[must_use]
    pub fn with_principal(mut self, principal: Principal) -> Self {
        self.principal = principal;
        self
    }
}

#[async_trait::async_trait]
impl AuditHook for FacadeAuditHook {
    async fn on_ask(&self, fields: AskAuditFields) {
        let outcome = if fields.success {
            AuditOutcome::Success
        } else {
            AuditOutcome::Error {
                kind: fields.failure_kind.unwrap_or_else(|| "unknown".into()),
            }
        };
        let mut event = AuditEvent::unsigned(
            fields.workspace_id,
            self.principal.clone(),
            AuditAction::AskComplete,
            AuditResource::Query {
                question_hash: fields.question_hash,
            },
            outcome,
            fields.adapter,
        );
        event.llm_provider = fields.llm_provider;
        event.llm_model = fields.llm_model;
        event.input_tokens = fields.input_tokens;
        event.output_tokens = fields.output_tokens;
        event.latency_ms = fields.latency_ms;
        let _ = self.writer.append(event).await;
    }

    async fn on_ingest(&self, fields: IngestAuditFields) {
        let mut event = AuditEvent::unsigned(
            fields.workspace_id,
            self.principal.clone(),
            AuditAction::Ingest,
            AuditResource::Document {
                doc_id: fields.doc_id,
            },
            if fields.success {
                AuditOutcome::Success
            } else {
                AuditOutcome::Error {
                    kind: "ingest_failed".into(),
                }
            },
            fields.adapter,
        );
        event.latency_ms = fields.latency_ms;
        event.input_tokens = u32::try_from(fields.byte_count / 4).unwrap_or(u32::MAX);
        event.output_tokens = fields.leaf_count;
        let _ = self.writer.append(event).await;
    }

    async fn on_delete(&self, fields: DeleteAuditFields) {
        let event = AuditEvent::unsigned(
            fields.workspace_id,
            self.principal.clone(),
            AuditAction::Delete,
            AuditResource::Document {
                doc_id: fields.doc_id,
            },
            if fields.success {
                AuditOutcome::Success
            } else {
                AuditOutcome::Error {
                    kind: "delete_failed".into(),
                }
            },
            fields.adapter,
        );
        let _ = self.writer.append(event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::MerkleBatch;
    use crate::sign::SigningSecret;
    use crate::writer::{AuditSink, WriterConfig};
    use pagebridge_core::workspace::WorkspaceId;
    use parking_lot::Mutex;

    struct Capture(Mutex<Vec<AuditAction>>);
    #[async_trait::async_trait]
    impl AuditSink for Capture {
        fn name(&self) -> &str {
            "capture"
        }
        async fn write_event(&self, e: &AuditEvent) -> crate::Result<()> {
            self.0.lock().push(e.action);
            Ok(())
        }
        async fn write_batch(&self, _b: &MerkleBatch) -> crate::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn hook_emits_one_event_per_call() {
        let mut writer = AuditWriter::new(SigningSecret::generate(), WriterConfig::default());
        let cap: Arc<Capture> = Arc::new(Capture(Mutex::new(Vec::new())));
        writer.add_sink(cap.clone());
        let hook = FacadeAuditHook::new(Arc::new(writer));
        let ws = WorkspaceId::new("acme").unwrap();
        hook.on_ask(AskAuditFields {
            workspace_id: ws.clone(),
            adapter: "embedded".into(),
            question_hash: "abc".into(),
            llm_provider: None,
            llm_model: None,
            input_tokens: 0,
            output_tokens: 0,
            latency_ms: 1,
            success: true,
            failure_kind: None,
        })
        .await;
        let actions = cap.0.lock().clone();
        assert_eq!(actions, vec![AuditAction::AskComplete]);
    }
}
