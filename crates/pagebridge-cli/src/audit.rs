//! CLI handlers for `pagebridge audit ...` subcommands.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use pagebridge_audit::{replay_chain, AuditEvent, SignatureVerifier};
use tokio::fs;

use crate::AuditCmd;

pub async fn run(action: AuditCmd, json: bool) -> Result<()> {
    match action {
        AuditCmd::Tail { dir, workspace, n } => {
            let path = dir.join(format!("{workspace}.events.ndjson"));
            tail(&path, n, json).await
        }
        AuditCmd::Verify {
            events,
            key,
            key_id,
        } => verify(&events, &key, &key_id, json).await,
        AuditCmd::Export { events, to } => {
            fs::copy(&events, &to).await?;
            if !json {
                println!("exported {} -> {}", events.display(), to.display());
            }
            Ok(())
        }
        AuditCmd::Sinks => {
            // The CLI does not yet read sink configuration from disk; we
            // print the static set the library can construct.
            let sinks = [
                "file",
                "worm",
                "tee",
                "splunk (http-sinks feature)",
                "elastic (http-sinks feature)",
            ];
            if json {
                println!("{}", serde_json::to_string(&sinks)?);
            } else {
                for s in sinks {
                    println!("{s}");
                }
            }
            Ok(())
        }
    }
}

async fn tail(path: &Path, n: usize, json: bool) -> Result<()> {
    let body = fs::read_to_string(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    let lines: Vec<&str> = body.lines().collect();
    let start = lines.len().saturating_sub(n);
    for l in &lines[start..] {
        if json {
            println!("{l}");
        } else {
            let event: AuditEvent = serde_json::from_str(l)?;
            println!(
                "{} {} {} {} {}",
                event.timestamp_ns,
                event.workspace_id,
                event.action.as_str(),
                event.adapter,
                event.event_id
            );
        }
    }
    Ok(())
}

async fn verify(events: &Path, key_path: &Path, key_id: &str, json: bool) -> Result<()> {
    let body = fs::read_to_string(events).await?;
    let mut parsed = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        parsed.push(serde_json::from_str::<AuditEvent>(line)?);
    }

    let raw = fs::read(key_path).await?;
    if raw.len() < 32 {
        return Err(anyhow!("public key file too short"));
    }
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(&raw[..32]);
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| anyhow!("parse pubkey: {e}"))?;
    let verifier = SignatureVerifier {
        key_id: key_id.to_string(),
        verifying,
    };
    let report = replay_chain(parsed.iter(), &verifier)?;

    if json {
        let payload = serde_json::json!({
            "ok": true,
            "events_seen": report.events_seen,
            "workspaces": report.workspaces,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "OK: {} events verified across workspaces {:?}",
            report.events_seen, report.workspaces
        );
    }
    Ok(())
}
