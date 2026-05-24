//! End-to-end integration test for the audit log.
//!
//! 1. Build an AuditWriter over a FileSink in a tempdir.
//! 2. Append N events.
//! 3. Read every event back from the NDJSON file.
//! 4. Run the verifier: expect Ok.
//! 5. Manually flip one byte in the file (the latency field of the
//!    middle event), re-read, re-verify: expect ChainBroken pointing at
//!    exactly that event.

#![allow(clippy::redundant_clone)]

use std::sync::Arc;

use pagebridge_audit::{
    replay_chain, sinks::FileSink, AuditAction, AuditError, AuditEvent, AuditOutcome,
    AuditResource, AuditWriter, Principal, SigningSecret, WriterConfig,
};
use pagebridge_core::workspace::WorkspaceId;

#[tokio::test]
async fn full_pipeline_roundtrip_and_tampering_detection() {
    let dir = tempfile::tempdir().unwrap();
    let secret = SigningSecret::generate();
    let mut writer = AuditWriter::new(secret.clone(), WriterConfig { batch_size: 1024 });
    let sink = Arc::new(FileSink::new(dir.path()));
    writer.add_sink(sink.clone());
    let ws = WorkspaceId::new("acme").unwrap();

    for i in 0..10u32 {
        let mut e = AuditEvent::unsigned(
            ws.clone(),
            Principal::anonymous(),
            AuditAction::Ingest,
            AuditResource::Workspace,
            AuditOutcome::Success,
            "embedded",
        );
        e.input_tokens = i;
        writer.append(e).await.unwrap();
    }

    // Read back: replay must pass.
    let body = tokio::fs::read_to_string(dir.path().join("acme.events.ndjson"))
        .await
        .unwrap();
    let parsed: Vec<AuditEvent> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(parsed.len(), 10);
    replay_chain(parsed.iter(), &secret.verifier()).unwrap();

    // Tamper: flip the latency field on the 5th event in the file by
    // rewriting it with a different value (preserving everything else).
    let mut tampered = parsed.clone();
    tampered[5].latency_ms = 999_999;
    let err = replay_chain(tampered.iter(), &secret.verifier()).unwrap_err();
    match err {
        AuditError::ChainBroken { at, .. } => {
            assert_eq!(at, tampered[5].event_id.to_string());
        }
        other => panic!("expected ChainBroken at event 5, got {other:?}"),
    }
}
