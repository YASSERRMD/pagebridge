//! `pagebridge verify-receipt` subcommand.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use pagebridge_receipt::{verify_receipt, AnswerReceipt, ReceiptVerifier};
use tokio::fs;

pub async fn run(receipt: &Path, key: &Path, key_id: &str, json: bool) -> Result<()> {
    let body = fs::read_to_string(receipt)
        .await
        .with_context(|| format!("read {}", receipt.display()))?;
    let parsed: AnswerReceipt =
        serde_json::from_str(&body).context("parse receipt JSON")?;
    let raw = fs::read(key).await.with_context(|| format!("read {}", key.display()))?;
    if raw.len() < 32 {
        return Err(anyhow!("public key file too short"));
    }
    let v = ReceiptVerifier::from_raw_bytes(key_id, &raw)
        .map_err(|e| anyhow!("verifier: {e}"))?;
    verify_receipt(&parsed, &v).map_err(|e| anyhow!("rejected: {e}"))?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "answer_id": parsed.answer_id,
                "key_id": parsed.key_id,
                "workspace_id": parsed.workspace_id,
            })
        );
    } else {
        println!(
            "OK: answer_id={} workspace={} key={}",
            parsed.answer_id, parsed.workspace_id, parsed.key_id
        );
    }
    Ok(())
}
