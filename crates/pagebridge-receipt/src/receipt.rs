//! `AnswerReceipt` plus the `issue_receipt` constructor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use pagebridge_audit::merkle::merkle_root;
use pagebridge_audit::sign::SigningSecret;
use pagebridge_audit::transparency::TrillianEntry;
use pagebridge_core::workspace::WorkspaceId;

use crate::error::Result;
use crate::fingerprint::{LlmFingerprint, NodeReference, PromptVersionMap};

/// Information used to verify a single answer end-to-end.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnswerReceipt {
    pub answer_id: String,
    pub workspace_id: WorkspaceId,
    pub question_hash_hex: String,
    pub answer_hash_hex: String,
    pub corpus_root_hex: String,
    pub used_nodes: Vec<NodeReference>,
    pub llm: LlmFingerprint,
    pub prompt_versions: PromptVersionMap,
    pub policy_versions: BTreeMap<String, u32>,
    pub trace_hash_hex: String,
    pub timestamp_ns: u128,
    pub signature_hex: String,
    pub key_id: String,
    pub transparency_log_entry: Option<TrillianEntry>,
}

/// Inputs the facade collects to mint a receipt.
#[derive(Debug, Clone)]
pub struct ReceiptInputs {
    pub answer_id: String,
    pub workspace_id: WorkspaceId,
    pub question: String,
    pub answer_text: String,
    pub used_nodes: Vec<NodeReference>,
    pub llm: LlmFingerprint,
    pub prompt_versions: PromptVersionMap,
    pub policy_versions: BTreeMap<String, u32>,
    pub trace_canonical: Vec<u8>,
    pub transparency_log_entry: Option<TrillianEntry>,
}

/// Compute the corpus_root from the set of `used_nodes`. A real
/// production deployment computes the root over the *entire* corpus and
/// proves inclusion of each used node; for the receipt's primary
/// purpose (proving the same nodes were available at a future
/// verification time), the per-answer root is sufficient and far
/// cheaper to compute.
#[must_use]
pub fn corpus_root_of(nodes: &[NodeReference]) -> [u8; 32] {
    let mut leaves: Vec<[u8; 32]> = nodes
        .iter()
        .map(|n| {
            let mut h = Sha256::new();
            h.update(n.node_id.as_str().as_bytes());
            h.update(b"|");
            h.update(n.content_hash_hex.as_bytes());
            h.update(b"|");
            h.update(n.version.to_be_bytes());
            let out = h.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&out);
            arr
        })
        .collect();
    // Sort leaves so identical node sets in different orders produce
    // the same root.
    leaves.sort();
    merkle_root(&leaves)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Construct a signed receipt from the facade's collected inputs.
pub fn issue_receipt(inputs: ReceiptInputs, secret: &SigningSecret) -> Result<AnswerReceipt> {
    let question_hash = sha256(inputs.question.as_bytes());
    let answer_hash = sha256(inputs.answer_text.as_bytes());
    let corpus_root = corpus_root_of(&inputs.used_nodes);
    let trace_hash = sha256(&inputs.trace_canonical);
    let timestamp_ns = now_ns();

    // The receipt body is everything except the signature itself; we
    // sign sha256 of the canonical encoding to keep the signed payload
    // a fixed 32 bytes.
    let mut body = AnswerReceipt {
        answer_id: inputs.answer_id,
        workspace_id: inputs.workspace_id,
        question_hash_hex: hex::encode(question_hash),
        answer_hash_hex: hex::encode(answer_hash),
        corpus_root_hex: hex::encode(corpus_root),
        used_nodes: inputs.used_nodes,
        llm: inputs.llm,
        prompt_versions: inputs.prompt_versions,
        policy_versions: inputs.policy_versions,
        trace_hash_hex: hex::encode(trace_hash),
        timestamp_ns,
        signature_hex: String::new(),
        key_id: secret.key_id.clone(),
        transparency_log_entry: inputs.transparency_log_entry,
    };
    let canonical = canonical_bytes(&body)?;
    let digest = sha256(&canonical);
    let sig = ed25519_dalek::Signer::sign(&secret.signing, &digest);
    body.signature_hex = hex::encode(sig.to_bytes());
    Ok(body)
}

/// Canonical bytes: serde_json::to_vec produces stable field order based
/// on the struct's declaration order. The signature field is held empty
/// during signing, then populated afterwards.
pub fn canonical_bytes(receipt: &AnswerReceipt) -> Result<Vec<u8>> {
    let mut clone = receipt.clone();
    clone.signature_hex = String::new();
    serde_json::to_vec(&clone).map_err(Into::into)
}

/// Recompute the hash that was signed for a receipt.
pub fn signing_digest(receipt: &AnswerReceipt) -> Result<[u8; 32]> {
    Ok(sha256(&canonical_bytes(receipt)?))
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::id::{DocId, NodeId};

    fn sample_inputs() -> ReceiptInputs {
        let doc = DocId::new("policy").unwrap();
        let n1 = NodeId::root(&doc).child("sec", "1").unwrap();
        ReceiptInputs {
            answer_id: "ans-001".into(),
            workspace_id: WorkspaceId::new("acme").unwrap(),
            question: "what is the limit?".into(),
            answer_text: "the limit is 5".into(),
            used_nodes: vec![NodeReference {
                node_id: n1,
                content_hash_hex: "deadbeef".into(),
                version: 1,
            }],
            llm: LlmFingerprint {
                provider: "anthropic".into(),
                model: "claude-haiku-4-5".into(),
                temperature_milli: 0,
                top_p_milli: 1000,
                seed: 42,
                revision: None,
            },
            prompt_versions: PromptVersionMap::new(),
            policy_versions: BTreeMap::new(),
            trace_canonical: b"trace".to_vec(),
            transparency_log_entry: None,
        }
    }

    #[test]
    fn issue_then_recompute_digest_matches() {
        let secret = SigningSecret::generate();
        let receipt = issue_receipt(sample_inputs(), &secret).unwrap();
        let digest = signing_digest(&receipt).unwrap();
        let sig =
            ed25519_dalek::Signature::from_slice(&hex::decode(&receipt.signature_hex).unwrap())
                .unwrap();
        ed25519_dalek::Verifier::verify(&secret.signing.verifying_key(), &digest, &sig).unwrap();
    }

    #[test]
    fn changing_answer_text_invalidates_signature() {
        let secret = SigningSecret::generate();
        let mut receipt = issue_receipt(sample_inputs(), &secret).unwrap();
        receipt.answer_hash_hex = "00".repeat(32);
        let digest = signing_digest(&receipt).unwrap();
        let sig =
            ed25519_dalek::Signature::from_slice(&hex::decode(&receipt.signature_hex).unwrap())
                .unwrap();
        let v = ed25519_dalek::Verifier::verify(
            &secret.signing.verifying_key(),
            &digest,
            &sig,
        );
        assert!(v.is_err());
    }

    #[test]
    fn corpus_root_is_node_order_independent() {
        let doc = DocId::new("doc").unwrap();
        let a = NodeReference {
            node_id: NodeId::root(&doc).child("sec", "a").unwrap(),
            content_hash_hex: "00".into(),
            version: 1,
        };
        let b = NodeReference {
            node_id: NodeId::root(&doc).child("sec", "b").unwrap(),
            content_hash_hex: "ff".into(),
            version: 1,
        };
        assert_eq!(corpus_root_of(&[a.clone(), b.clone()]), corpus_root_of(&[b, a]));
    }
}
