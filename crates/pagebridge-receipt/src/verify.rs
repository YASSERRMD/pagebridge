//! Offline receipt verification.
//!
//! Given a receipt JSON file plus the matching Ed25519 public key, the
//! verifier:
//!
//! 1. Recomputes the signing digest (sha256 over canonical JSON with
//!    `signature_hex` zeroed).
//! 2. Confirms the digest matches the embedded signature.
//! 3. Recomputes the `corpus_root_hex` from the embedded `used_nodes`
//!    and confirms it matches.
//!
//! No pagebridge process is required to verify. The verifier itself is
//! tiny (no async dependencies) so it can be run on an air-gapped
//! auditor workstation.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::error::{ReceiptError, Result};
use crate::receipt::{corpus_root_of, signing_digest, AnswerReceipt};

pub struct ReceiptVerifier {
    pub key_id: String,
    pub verifying: VerifyingKey,
}

impl ReceiptVerifier {
    pub fn from_raw_bytes(key_id: impl Into<String>, raw: &[u8]) -> Result<Self> {
        if raw.len() < 32 {
            return Err(ReceiptError::Signature("public key too short".into()));
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&raw[..32]);
        let verifying = VerifyingKey::from_bytes(&a)
            .map_err(|e| ReceiptError::Signature(format!("parse pubkey: {e}")))?;
        Ok(Self {
            key_id: key_id.into(),
            verifying,
        })
    }
}

/// Verify a single receipt against a known public key.
pub fn verify_receipt(receipt: &AnswerReceipt, verifier: &ReceiptVerifier) -> Result<()> {
    if receipt.key_id != verifier.key_id {
        return Err(ReceiptError::Rejected(format!(
            "receipt key_id {} != verifier {}",
            receipt.key_id, verifier.key_id
        )));
    }
    let digest = signing_digest(receipt)?;
    let sig_bytes = hex::decode(&receipt.signature_hex)
        .map_err(|e| ReceiptError::Canonical(format!("signature hex: {e}")))?;
    let sig = Signature::from_slice(&sig_bytes)
        .map_err(|e| ReceiptError::Signature(format!("signature parse: {e}")))?;
    verifier
        .verifying
        .verify(&digest, &sig)
        .map_err(|e| ReceiptError::Rejected(format!("signature invalid: {e}")))?;
    let computed_root = corpus_root_of(&receipt.used_nodes);
    let claimed = hex::decode(&receipt.corpus_root_hex)
        .map_err(|e| ReceiptError::Canonical(format!("root hex: {e}")))?;
    if claimed != computed_root {
        return Err(ReceiptError::Rejected(
            "corpus_root does not match recomputation from used_nodes".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::{LlmFingerprint, NodeReference, PromptVersionMap};
    use crate::receipt::{issue_receipt, ReceiptInputs};
    use pagebridge_audit::sign::SigningSecret;
    use pagebridge_core::id::{DocId, NodeId};
    use pagebridge_core::workspace::WorkspaceId;
    use std::collections::BTreeMap;

    fn inputs() -> ReceiptInputs {
        let doc = DocId::new("doc").unwrap();
        ReceiptInputs {
            answer_id: "a".into(),
            workspace_id: WorkspaceId::new("acme").unwrap(),
            question: "q".into(),
            answer_text: "a".into(),
            used_nodes: vec![NodeReference {
                node_id: NodeId::root(&doc).child("sec", "1").unwrap(),
                content_hash_hex: "00".into(),
                version: 1,
            }],
            llm: LlmFingerprint {
                provider: "openai".into(),
                model: "gpt-4o-mini".into(),
                temperature_milli: 0,
                top_p_milli: 1000,
                seed: 0,
                revision: None,
            },
            prompt_versions: PromptVersionMap::new(),
            policy_versions: BTreeMap::new(),
            trace_canonical: vec![],
            transparency_log_entry: None,
        }
    }

    #[test]
    fn happy_path_verifies() {
        let secret = SigningSecret::generate();
        let receipt = issue_receipt(inputs(), &secret).unwrap();
        let v = ReceiptVerifier {
            key_id: secret.key_id.clone(),
            verifying: secret.signing.verifying_key(),
        };
        verify_receipt(&receipt, &v).unwrap();
    }

    #[test]
    fn tampered_answer_hash_rejected() {
        let secret = SigningSecret::generate();
        let mut receipt = issue_receipt(inputs(), &secret).unwrap();
        receipt.answer_hash_hex = "ff".repeat(32);
        let v = ReceiptVerifier {
            key_id: secret.key_id.clone(),
            verifying: secret.signing.verifying_key(),
        };
        assert!(verify_receipt(&receipt, &v).is_err());
    }

    #[test]
    fn tampered_used_nodes_rejected() {
        let secret = SigningSecret::generate();
        let mut receipt = issue_receipt(inputs(), &secret).unwrap();
        receipt.used_nodes[0].content_hash_hex = "ff".into();
        let v = ReceiptVerifier {
            key_id: secret.key_id.clone(),
            verifying: secret.signing.verifying_key(),
        };
        assert!(verify_receipt(&receipt, &v).is_err());
    }
}
