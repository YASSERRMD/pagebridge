//! Local NDJSON file sink. Appends one JSON line per event and per batch,
//! to separate files so tools can tail events or anchors independently.
//!
//! Files are opened lazily and never truncated. Callers wanting size-bound
//! rotation should run a log shipper (logrotate, vector, fluent-bit) over
//! the directory; this sink does not rotate to keep the append path simple
//! and chain-safe.

use std::path::{Path, PathBuf};

use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;
use crate::merkle::MerkleBatch;
use crate::writer::AuditSink;

pub struct FileSink {
    base: PathBuf,
    name: String,
    write_lock: Mutex<()>,
}

impl FileSink {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            name: "file".to_string(),
            write_lock: Mutex::new(()),
        }
    }

    fn events_path(&self, workspace: &str) -> PathBuf {
        self.base.join(format!("{workspace}.events.ndjson"))
    }

    fn batches_path(&self, workspace: &str) -> PathBuf {
        self.base.join(format!("{workspace}.batches.ndjson"))
    }

    async fn ensure_dir(&self) -> Result<()> {
        create_dir_all(&self.base).await.map_err(|e| AuditError::Sink {
            sink: self.name.clone(),
            message: format!("mkdir: {e}"),
        })
    }
}

#[async_trait::async_trait]
impl AuditSink for FileSink {
    fn name(&self) -> &str {
        &self.name
    }

    async fn write_event(&self, event: &AuditEvent) -> Result<()> {
        self.ensure_dir().await?;
        let path = self.events_path(event.workspace_id.as_str());
        let line = serde_json::to_vec(event)?;
        write_line(&path, &line, &self.write_lock).await
    }

    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
        self.ensure_dir().await?;
        let path = self.batches_path(&batch.workspace_id);
        let line = serde_json::to_vec(batch)?;
        write_line(&path, &line, &self.write_lock).await
    }
}

async fn write_line(path: &Path, payload: &[u8], lock: &Mutex<()>) -> Result<()> {
    let _g = lock.lock().await;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(AuditError::Io)?;
    f.write_all(payload).await?;
    f.write_all(b"\n").await?;
    f.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditAction, AuditOutcome, AuditResource, Principal};
    use crate::sign::SigningSecret;
    use crate::writer::{AuditWriter, WriterConfig};
    use pagebridge_core::workspace::WorkspaceId;
    use std::sync::Arc;

    #[tokio::test]
    async fn writes_ndjson_lines_for_events_and_batch() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Arc::new(FileSink::new(dir.path()));
        let mut writer = AuditWriter::new(
            SigningSecret::generate(),
            WriterConfig { batch_size: 3 },
        );
        writer.add_sink(sink.clone());

        let ws = WorkspaceId::new("acme").unwrap();
        for i in 0..3 {
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

        let events_path = dir.path().join("acme.events.ndjson");
        let batches_path = dir.path().join("acme.batches.ndjson");
        let events = tokio::fs::read_to_string(events_path).await.unwrap();
        let batches = tokio::fs::read_to_string(batches_path).await.unwrap();
        assert_eq!(events.lines().count(), 3);
        assert_eq!(batches.lines().count(), 1);
    }
}
