//! End-to-end: issue a receipt from the FacadeReceiptIssuer, serialize
//! it to JSON, parse it back, run verify_receipt with the matching
//! public key, and confirm tampering breaks verification.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_audit::sign::SigningSecret;
use pagebridge_core::audit_hook::{ReceiptIssuanceInputs, ReceiptIssuer};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::workspace::WorkspaceId;
use pagebridge_receipt::{
    verify_receipt, AnswerReceipt, FacadeReceiptIssuer, LlmFingerprint, ReceiptVerifier,
};

fn make_issuer() -> (Arc<SigningSecret>, FacadeReceiptIssuer) {
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
    (secret, issuer)
}

#[tokio::test]
async fn facade_issued_receipt_verifies() {
    let (secret, issuer) = make_issuer();
    let doc = DocId::new("doc").unwrap();
    let n1 = NodeId::root(&doc).child("sec", "1").unwrap();
    let value = issuer
        .issue(ReceiptIssuanceInputs {
            workspace_id: WorkspaceId::new("acme").unwrap(),
            question: "the question".into(),
            answer_text: "the answer".into(),
            used_node_ids: vec![n1.clone()],
            used_node_content_hashes: vec!["deadbeef".into()],
            prompt_versions: Default::default(),
        })
        .await
        .expect("issuer returned None");

    let receipt: AnswerReceipt = serde_json::from_value(value).unwrap();
    let v = ReceiptVerifier {
        key_id: secret.key_id.clone(),
        verifying: secret.signing.verifying_key(),
    };
    verify_receipt(&receipt, &v).unwrap();
}

#[tokio::test]
async fn tampered_field_breaks_verification() {
    let (secret, issuer) = make_issuer();
    let doc = DocId::new("doc").unwrap();
    let value = issuer
        .issue(ReceiptIssuanceInputs {
            workspace_id: WorkspaceId::new("acme").unwrap(),
            question: "q".into(),
            answer_text: "a".into(),
            used_node_ids: vec![NodeId::root(&doc)],
            used_node_content_hashes: vec!["00".into()],
            prompt_versions: Default::default(),
        })
        .await
        .unwrap();
    let mut receipt: AnswerReceipt = serde_json::from_value(value).unwrap();
    receipt.answer_hash_hex = "ff".repeat(32);
    let v = ReceiptVerifier {
        key_id: secret.key_id.clone(),
        verifying: secret.signing.verifying_key(),
    };
    assert!(verify_receipt(&receipt, &v).is_err());
}
