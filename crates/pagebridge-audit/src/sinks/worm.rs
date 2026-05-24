//! WormFileSink: append-only sink that refuses to overwrite existing rows.
//!
//! This is a local stand-in for an S3 object-lock or Azure immutable-blob
//! target. The on-disk format is identical to [`crate::sinks::file::FileSink`]
//! (NDJSON, one record per line) but the writer asserts that every record
//! it appends is *new*: if the same `event_id` (or `batch_id`) appears
//! twice, the sink returns an error rather than silently appending.
//!
//! Production deployments swap this for an actual WORM bucket; the
//! semantics surface remains identical.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;
use crate::merkle::MerkleBatch;
use crate::writer::AuditSink;

pub struct WormFileSink {
    base: PathBuf,
    name: String,
    seen: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    events: HashSet<String>,
    batches: HashSet<(String, u64)>,
}

impl WormFileSink {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            name: "worm".to_string(),
            seen: Arc::new(Mutex::new(Inner::default())),
        }
    }

    fn events_path(&self, workspace: &str) -> PathBuf {
        self.base.join(format!("{workspace}.events.worm.ndjson"))
    }

    fn batches_path(&self, workspace: &str) -> PathBuf {
        self.base.join(format!("{workspace}.batches.worm.ndjson"))
    }
}

#[async_trait::async_trait]
impl AuditSink for WormFileSink {
    fn name(&self) -> &str {
        &self.name
    }

    async fn write_event(&self, event: &AuditEvent) -> Result<()> {
        create_dir_all(&self.base).await?;
        let event_id = event.event_id.to_string();
        let inserted = {
            let mut inner = self.seen.lock();
            inner.events.insert(event_id.clone())
        };
        if !inserted {
            return Err(AuditError::Sink {
                sink: self.name.clone(),
                message: format!("event {event_id} already written (WORM violation)"),
            });
        }
        let line = serde_json::to_vec(event)?;
        let path = self.events_path(event.workspace_id.as_str());
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(&line).await?;
        f.write_all(b"\n").await?;
        f.flush().await?;
        Ok(())
    }

    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
        create_dir_all(&self.base).await?;
        let key = (batch.workspace_id.clone(), batch.batch_id);
        let inserted = {
            let mut inner = self.seen.lock();
            inner.batches.insert(key.clone())
        };
        if !inserted {
            return Err(AuditError::Sink {
                sink: self.name.clone(),
                message: format!(
                    "batch {}::{} already written (WORM violation)",
                    key.0, key.1
                ),
            });
        }
        let line = serde_json::to_vec(batch)?;
        let path = self.batches_path(&batch.workspace_id);
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(&line).await?;
        f.write_all(b"\n").await?;
        f.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditAction, AuditOutcome, AuditResource, Principal};
    use pagebridge_core::workspace::WorkspaceId;

    #[tokio::test]
    async fn second_write_of_same_event_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let sink = WormFileSink::new(dir.path());
        let ws = WorkspaceId::new("acme").unwrap();
        let e = AuditEvent::unsigned(
            ws,
            Principal::anonymous(),
            AuditAction::AskStart,
            AuditResource::Workspace,
            AuditOutcome::Success,
            "embedded",
        );
        sink.write_event(&e).await.unwrap();
        assert!(sink.write_event(&e).await.is_err());
    }
}
