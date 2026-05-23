//! Replay a chain of audit events and report the first inconsistency.
//!
//! Given an ordered iterator of [`AuditEvent`]s for one workspace plus the
//! matching [`SignatureVerifier`], [`replay_chain`] walks the chain,
//! verifies each signature, checks each `prev_hash` against the prior
//! event's `event_hash`, and returns either `Ok(report)` with stats or
//! `Err(ChainBroken { at, detail })` pointing at the first bad event.
//!
//! Used by the CLI `pagebridge audit verify` subcommand, by automated
//! integrity checks in CI, and by any downstream auditor.

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;
use crate::merkle::{merkle_root, MerkleBatch};
use crate::sign::{verify_event, SignatureVerifier};

#[derive(Debug, Default)]
pub struct ReplayReport {
    pub events_seen: u64,
    pub batches_seen: u64,
    pub workspaces: Vec<String>,
}

/// Verify every event in `events`, in order. Returns the first chain or
/// signature failure if any; otherwise a summary.
pub fn replay_chain<'a, I>(events: I, verifier: &SignatureVerifier) -> Result<ReplayReport>
where
    I: IntoIterator<Item = &'a AuditEvent>,
{
    let mut report = ReplayReport::default();
    let mut prev_hash = [0u8; 32];
    let mut current_workspace: Option<String> = None;

    for event in events {
        let ws = event.workspace_id.as_str().to_string();
        if current_workspace.as_deref() != Some(&ws) {
            current_workspace = Some(ws.clone());
            report.workspaces.push(ws);
            prev_hash = [0u8; 32];
        }
        if event.prev_hash != prev_hash {
            return Err(AuditError::ChainBroken {
                at: event.event_id.to_string(),
                detail: format!(
                    "prev_hash mismatch: expected {}, found {}",
                    hex::encode(prev_hash),
                    hex::encode(event.prev_hash)
                ),
            });
        }
        verify_event(event, verifier)?;
        prev_hash = event.event_hash;
        report.events_seen += 1;
    }
    Ok(report)
}

/// Verify a single Merkle batch by recomputing the root from the leaf
/// hashes the caller already has. Returns `Ok(())` if the recomputed root
/// matches `batch.root`.
pub fn verify_batch(batch: &MerkleBatch, leaves: &[[u8; 32]]) -> Result<()> {
    let leaf_count = u32::try_from(leaves.len()).unwrap_or(u32::MAX);
    if leaf_count != batch.leaf_count {
        return Err(AuditError::MerkleProof {
            batch: batch.batch_id,
            detail: format!(
                "leaf count mismatch: have {leaf_count}, batch claims {}",
                batch.leaf_count
            ),
        });
    }
    let root = merkle_root(leaves);
    if root != batch.root {
        return Err(AuditError::MerkleProof {
            batch: batch.batch_id,
            detail: format!(
                "root mismatch: computed {}, batch {}",
                hex::encode(root),
                hex::encode(batch.root)
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditAction, AuditOutcome, AuditResource, Principal};
    use crate::sign::SigningSecret;
    use crate::writer::{AuditSink, AuditWriter, WriterConfig};
    use pagebridge_core::workspace::WorkspaceId;
    use std::sync::Arc;

    struct Capture {
        events: parking_lot::Mutex<Vec<AuditEvent>>,
        batches: parking_lot::Mutex<Vec<MerkleBatch>>,
    }
    impl Capture {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Default::default(),
                batches: Default::default(),
            })
        }
    }
    #[async_trait::async_trait]
    impl AuditSink for Capture {
        fn name(&self) -> &str {
            "cap"
        }
        async fn write_event(&self, e: &AuditEvent) -> Result<()> {
            self.events.lock().push(e.clone());
            Ok(())
        }
        async fn write_batch(&self, b: &MerkleBatch) -> Result<()> {
            self.batches.lock().push(b.clone());
            Ok(())
        }
    }

    async fn build_chain(n: u32) -> (SigningSecret, Vec<AuditEvent>, Vec<MerkleBatch>) {
        let secret = SigningSecret::generate();
        let mut writer = AuditWriter::new(secret.clone(), WriterConfig { batch_size: n });
        let sink = Capture::new();
        writer.add_sink(sink.clone());
        let ws = WorkspaceId::new("acme").unwrap();
        for i in 0..n {
            let mut e = AuditEvent::unsigned(
                ws.clone(),
                Principal::anonymous(),
                AuditAction::AskStart,
                AuditResource::Workspace,
                AuditOutcome::Success,
                "embedded",
            );
            e.input_tokens = i;
            writer.append(e).await.unwrap();
        }
        let events = sink.events.lock().clone();
        let batches = sink.batches.lock().clone();
        (secret, events, batches)
    }

    #[tokio::test]
    async fn replay_passes_for_clean_chain() {
        let (secret, events, _) = build_chain(8).await;
        let report = replay_chain(&events, &secret.verifier()).unwrap();
        assert_eq!(report.events_seen, 8);
    }

    #[tokio::test]
    async fn tampering_is_detected_at_modified_event() {
        let (secret, mut events, _) = build_chain(6).await;
        events[3].latency_ms = 999_999;
        let err = replay_chain(&events, &secret.verifier()).unwrap_err();
        match err {
            AuditError::ChainBroken { at, .. } => {
                assert_eq!(at, events[3].event_id.to_string());
            }
            other => panic!("expected ChainBroken, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_recompute_matches_writer_root() {
        let (_secret, events, batches) = build_chain(4).await;
        let leaves: Vec<[u8; 32]> = events.iter().map(|e| e.event_hash).collect();
        verify_batch(&batches[0], &leaves).unwrap();
    }
}
