//! Ed25519 signing for audit events plus the hash-chain helper.
//!
//! Every event's `event_hash` is the sha256 of the canonical JSON of the
//! event with `event_hash` and `signature` zeroed. The signature is over
//! `event_hash` itself, not the full body, so verifiers can detach a single
//! 32-byte hash and verify it without re-canonicalising the event.

use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;

/// A signing key for audit events. Wraps an Ed25519 keypair and keeps an
/// identifier (`key_id`) that is written into each event so verifiers can
/// pick the right public key during replay.
#[derive(Clone)]
pub struct SigningSecret {
    pub key_id: String,
    pub signing: SigningKey,
}

impl SigningSecret {
    /// Generate a fresh signing key with a random 8-byte hex key id.
    #[must_use]
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing.verifying_key().to_bytes();
        let mut h = Sha256::new();
        h.update(pub_bytes);
        let digest = h.finalize();
        let key_id = hex::encode(&digest[..8]);
        Self { key_id, signing }
    }

    /// Construct from a 32-byte secret and an explicit key id.
    pub fn from_bytes(key_id: impl Into<String>, secret: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&secret);
        Self {
            key_id: key_id.into(),
            signing,
        }
    }

    /// Public verifier matched to this secret.
    #[must_use]
    pub fn verifier(&self) -> SignatureVerifier {
        SignatureVerifier {
            key_id: self.key_id.clone(),
            verifying: self.signing.verifying_key(),
        }
    }

    /// Load a 32-byte raw secret from disk (legacy / dev format).
    pub async fn load_from_file(path: &Path) -> Result<Self> {
        let bytes = tokio::fs::read(path).await?;
        if bytes.len() < 32 {
            return Err(AuditError::Signature(format!(
                "signing key at {} too short: {} bytes",
                path.display(),
                bytes.len()
            )));
        }
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes[..32]);
        let key_id = hex::encode(&bytes[..8]);
        Ok(Self::from_bytes(key_id, secret))
    }

    /// Write the 32-byte secret to disk. Caller is responsible for permissions.
    pub async fn save_to_file(&self, path: &Path) -> Result<()> {
        tokio::fs::write(path, self.signing.to_bytes()).await?;
        Ok(())
    }
}

/// A public verifier; cheap to clone and safe to share across threads.
#[derive(Clone)]
pub struct SignatureVerifier {
    pub key_id: String,
    pub verifying: VerifyingKey,
}

impl SignatureVerifier {
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<()> {
        let sig = Signature::from_slice(signature)
            .map_err(|e| AuditError::Signature(format!("malformed signature: {e}")))?;
        self.verifying
            .verify(message, &sig)
            .map_err(|e| AuditError::Signature(format!("invalid signature: {e}")))
    }
}

/// Compute the canonical sha256 of an event with `event_hash` and
/// `signature` zeroed. Used both when sealing an event and when verifying.
pub fn canonical_event_hash(event: &AuditEvent) -> Result<[u8; 32]> {
    let mut stripped = event.clone();
    stripped.event_hash = [0u8; 32];
    stripped.signature.clear();
    let bytes = canonical_json(&stripped)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    Ok(arr)
}

/// Serialize to canonical JSON. We rely on serde_json's deterministic
/// rendering for primitive types plus a stable field ordering (struct
/// fields are emitted in declaration order, which we treat as canonical).
fn canonical_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(Into::into)
}

/// Seal an event in place: chain it to `prev_hash`, compute `event_hash`,
/// sign, and stamp the `key_id`.
pub fn seal_event(
    event: &mut AuditEvent,
    prev_hash: [u8; 32],
    secret: &SigningSecret,
) -> Result<()> {
    event.prev_hash = prev_hash;
    event.key_id = secret.key_id.clone();
    event.event_hash = canonical_event_hash(event)?;
    let sig = secret.signing.sign(&event.event_hash);
    event.signature = sig.to_bytes().to_vec();
    Ok(())
}

/// Verify a single event: recompute the canonical hash and check the
/// signature against the supplied verifier.
pub fn verify_event(event: &AuditEvent, verifier: &SignatureVerifier) -> Result<()> {
    if event.key_id != verifier.key_id {
        return Err(AuditError::Signature(format!(
            "event key_id {} does not match verifier {}",
            event.key_id, verifier.key_id
        )));
    }
    let recomputed = canonical_event_hash(event)?;
    if recomputed != event.event_hash {
        return Err(AuditError::ChainBroken {
            at: event.event_id.to_string(),
            detail: "event_hash does not match canonical recomputation".into(),
        });
    }
    verifier.verify(&event.event_hash, &event.signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditAction, AuditOutcome, AuditResource, Principal};
    use pagebridge_core::workspace::WorkspaceId;

    fn sample_event() -> AuditEvent {
        AuditEvent::unsigned(
            WorkspaceId::new("acme").unwrap(),
            Principal::anonymous(),
            AuditAction::AskStart,
            AuditResource::Query {
                question_hash: "abc".into(),
            },
            AuditOutcome::Success,
            "embedded",
        )
    }

    #[test]
    fn seal_and_verify_round_trip() {
        let secret = SigningSecret::generate();
        let mut e = sample_event();
        seal_event(&mut e, [0u8; 32], &secret).unwrap();
        verify_event(&e, &secret.verifier()).unwrap();
    }

    #[test]
    fn tampered_event_fails_verification() {
        let secret = SigningSecret::generate();
        let mut e = sample_event();
        seal_event(&mut e, [0u8; 32], &secret).unwrap();
        // Mutate the latency field and re-verify; signature must reject.
        e.latency_ms = e.latency_ms.wrapping_add(1);
        assert!(verify_event(&e, &secret.verifier()).is_err());
    }

    #[test]
    fn wrong_key_id_rejected() {
        let secret = SigningSecret::generate();
        let other = SigningSecret::generate();
        let mut e = sample_event();
        seal_event(&mut e, [0u8; 32], &secret).unwrap();
        assert!(verify_event(&e, &other.verifier()).is_err());
    }
}
