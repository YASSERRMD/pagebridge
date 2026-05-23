//! `FacadeReceiptIssuer`: bridges the core `ReceiptIssuer` trait to the
//! signing key. Pass one to `PagebridgeOptions::with_receipt_issuer` and
//! every `ask` returns an `Answer` whose `receipt_json` is a signed
//! `AnswerReceipt`.

use std::sync::Arc;

use pagebridge_audit::sign::SigningSecret;
use pagebridge_audit::transparency::TrillianEntry;
use pagebridge_core::audit_hook::{ReceiptIssuanceInputs, ReceiptIssuer};

use crate::fingerprint::{LlmFingerprint, NodeReference};
use crate::receipt::{issue_receipt, ReceiptInputs};

pub struct FacadeReceiptIssuer {
    secret: Arc<SigningSecret>,
    /// Sampling parameters that were active when the LLM ran. The hook
    /// does not yet receive these from the facade, so we record them
    /// here and the same fingerprint is attached to every receipt.
    llm: LlmFingerprint,
    transparency: Option<TrillianEntry>,
}

impl FacadeReceiptIssuer {
    #[must_use]
    pub fn new(secret: Arc<SigningSecret>, llm: LlmFingerprint) -> Self {
        Self {
            secret,
            llm,
            transparency: None,
        }
    }

    #[must_use]
    pub fn with_transparency(mut self, entry: TrillianEntry) -> Self {
        self.transparency = Some(entry);
        self
    }
}

#[async_trait::async_trait]
impl ReceiptIssuer for FacadeReceiptIssuer {
    async fn issue(&self, inputs: ReceiptIssuanceInputs) -> Option<serde_json::Value> {
        let nodes: Vec<NodeReference> = inputs
            .used_node_ids
            .into_iter()
            .zip(inputs.used_node_content_hashes.into_iter())
            .map(|(node_id, content_hash_hex)| NodeReference {
                node_id,
                content_hash_hex,
                version: 1,
            })
            .collect();
        let receipt_inputs = ReceiptInputs {
            answer_id: ulid::Ulid::new().to_string(),
            workspace_id: inputs.workspace_id,
            question: inputs.question,
            answer_text: inputs.answer_text,
            used_nodes: nodes,
            llm: self.llm.clone(),
            prompt_versions: inputs.prompt_versions,
            policy_versions: std::collections::BTreeMap::new(),
            trace_canonical: vec![],
            transparency_log_entry: self.transparency.clone(),
        };
        match issue_receipt(receipt_inputs, &self.secret) {
            Ok(receipt) => serde_json::to_value(&receipt).ok(),
            Err(e) => {
                tracing::warn!("receipt issuance failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::id::{DocId, NodeId};
    use pagebridge_core::workspace::WorkspaceId;

    #[tokio::test]
    async fn issuer_returns_signed_json() {
        let secret = Arc::new(SigningSecret::generate());
        let llm = LlmFingerprint {
            provider: "anthropic".into(),
            model: "claude-haiku-4-5".into(),
            temperature_milli: 0,
            top_p_milli: 1000,
            seed: 7,
            revision: None,
        };
        let issuer = FacadeReceiptIssuer::new(secret.clone(), llm);
        let doc = DocId::new("doc").unwrap();
        let n = NodeId::root(&doc);
        let out = issuer
            .issue(ReceiptIssuanceInputs {
                workspace_id: WorkspaceId::new("acme").unwrap(),
                question: "hi".into(),
                answer_text: "hello".into(),
                used_node_ids: vec![n],
                used_node_content_hashes: vec!["00".into()],
                prompt_versions: Default::default(),
            })
            .await
            .unwrap();
        assert!(out.is_object());
        assert!(out["signature_hex"].as_str().is_some());
        assert_eq!(out["key_id"], serde_json::Value::String(secret.key_id.clone()));
    }
}
