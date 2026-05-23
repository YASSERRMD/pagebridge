//! Append-only audit writer with batch-anchor support.
//!
//! The writer wraps:
//!   - a signing key,
//!   - a list of pluggable [`AuditSink`]s,
//!   - per-workspace chain state (last `event_hash`),
//!   - a rolling buffer of unsealed event hashes used to compute the next
//!     Merkle batch root.
//!
//! Callers obtain a writer via [`AuditWriter::new`] and call
//! [`AuditWriter::append`] for every event. When the per-workspace buffer
//! reaches `batch_size`, a Merkle root is computed and broadcast to every
//! sink as a [`MerkleBatch`].

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use pagebridge_core::workspace::WorkspaceId;

use crate::error::Result;
use crate::event::AuditEvent;
use crate::merkle::{merkle_root, MerkleBatch};
use crate::sign::{seal_event, SigningSecret};

/// A pluggable destination for sealed audit events and Merkle batches.
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn write_event(&self, event: &AuditEvent) -> Result<()>;
    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct WriterConfig {
    pub batch_size: u32,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self { batch_size: 1024 }
    }
}

struct ChainState {
    last_hash: [u8; 32],
    batch_id: u64,
    pending: Vec<[u8; 32]>,
    first_event_id: Option<String>,
    last_event_id: Option<String>,
}

impl ChainState {
    fn new() -> Self {
        Self {
            last_hash: [0u8; 32],
            batch_id: 0,
            pending: Vec::new(),
            first_event_id: None,
            last_event_id: None,
        }
    }
}

pub struct AuditWriter {
    secret: Arc<SigningSecret>,
    sinks: Vec<Arc<dyn AuditSink>>,
    cfg: WriterConfig,
    chains: Mutex<HashMap<WorkspaceId, ChainState>>,
}

impl AuditWriter {
    #[must_use]
    pub fn new(secret: SigningSecret, cfg: WriterConfig) -> Self {
        Self {
            secret: Arc::new(secret),
            sinks: Vec::new(),
            cfg,
            chains: Mutex::new(HashMap::new()),
        }
    }

    pub fn add_sink(&mut self, sink: Arc<dyn AuditSink>) {
        self.sinks.push(sink);
    }

    /// Seal `event` against the workspace's chain, publish to sinks, and
    /// flush a Merkle batch if the buffer is full. Returns the sealed
    /// `event_hash` for callers that want to record it elsewhere.
    pub async fn append(&self, mut event: AuditEvent) -> Result<[u8; 32]> {
        let workspace = event.workspace_id.clone();
        let (prev_hash, flush_batch) = {
            let mut chains = self.chains.lock();
            let state = chains.entry(workspace.clone()).or_insert_with(ChainState::new);
            (state.last_hash, false)
        };
        let _ = flush_batch;

        seal_event(&mut event, prev_hash, &self.secret)?;
        let sealed_hash = event.event_hash;

        let maybe_batch = {
            let mut chains = self.chains.lock();
            let state = chains.entry(workspace.clone()).or_insert_with(ChainState::new);
            state.last_hash = sealed_hash;
            if state.first_event_id.is_none() {
                state.first_event_id = Some(event.event_id.to_string());
            }
            state.last_event_id = Some(event.event_id.to_string());
            state.pending.push(sealed_hash);
            if state.pending.len() as u32 >= self.cfg.batch_size {
                let leaves = std::mem::take(&mut state.pending);
                let root = merkle_root(&leaves);
                let batch_id = state.batch_id;
                state.batch_id += 1;
                let first = state.first_event_id.take().unwrap_or_default();
                let last = state.last_event_id.clone().unwrap_or_default();
                Some(MerkleBatch {
                    batch_id,
                    workspace_id: workspace.as_str().to_string(),
                    first_event_id: first,
                    last_event_id: last,
                    leaf_count: u32::try_from(leaves.len()).unwrap_or(u32::MAX),
                    root,
                })
            } else {
                None
            }
        };

        for sink in &self.sinks {
            sink.write_event(&event).await?;
        }
        if let Some(batch) = maybe_batch {
            for sink in &self.sinks {
                sink.write_batch(&batch).await?;
            }
        }

        Ok(sealed_hash)
    }

    /// Force the per-workspace buffer to be sealed into a Merkle batch and
    /// broadcast to every sink. Idempotent on empty buffers.
    pub async fn flush(&self, workspace: &WorkspaceId) -> Result<Option<MerkleBatch>> {
        let maybe_batch = {
            let mut chains = self.chains.lock();
            let state = chains.entry(workspace.clone()).or_insert_with(ChainState::new);
            if state.pending.is_empty() {
                return Ok(None);
            }
            let leaves = std::mem::take(&mut state.pending);
            let root = merkle_root(&leaves);
            let batch_id = state.batch_id;
            state.batch_id += 1;
            let first = state.first_event_id.take().unwrap_or_default();
            let last = state.last_event_id.clone().unwrap_or_default();
            Some(MerkleBatch {
                batch_id,
                workspace_id: workspace.as_str().to_string(),
                first_event_id: first,
                last_event_id: last,
                leaf_count: u32::try_from(leaves.len()).unwrap_or(u32::MAX),
                root,
            })
        };
        if let Some(batch) = maybe_batch.as_ref() {
            for sink in &self.sinks {
                sink.write_batch(batch).await?;
            }
        }
        Ok(maybe_batch)
    }

    #[must_use]
    pub fn signing_key_id(&self) -> &str {
        &self.secret.key_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditAction, AuditOutcome, AuditResource, Principal};

    struct CapturingSink {
        events: parking_lot::Mutex<Vec<AuditEvent>>,
        batches: parking_lot::Mutex<Vec<MerkleBatch>>,
    }

    impl CapturingSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: parking_lot::Mutex::new(Vec::new()),
                batches: parking_lot::Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl AuditSink for CapturingSink {
        fn name(&self) -> &str {
            "capture"
        }
        async fn write_event(&self, event: &AuditEvent) -> Result<()> {
            self.events.lock().push(event.clone());
            Ok(())
        }
        async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
            self.batches.lock().push(batch.clone());
            Ok(())
        }
    }

    fn make_event(ws: &WorkspaceId, n: u32) -> AuditEvent {
        let mut e = AuditEvent::unsigned(
            ws.clone(),
            Principal::anonymous(),
            AuditAction::AskStart,
            AuditResource::Query {
                question_hash: format!("q-{n}"),
            },
            AuditOutcome::Success,
            "embedded",
        );
        e.input_tokens = n;
        e
    }

    #[tokio::test]
    async fn chains_link_one_to_the_next() {
        let secret = SigningSecret::generate();
        let mut writer = AuditWriter::new(
            secret.clone(),
            WriterConfig { batch_size: 16 },
        );
        let sink = CapturingSink::new();
        writer.add_sink(sink.clone());
        let ws = WorkspaceId::new("acme").unwrap();
        let mut last = [0u8; 32];
        for i in 0..5 {
            let h = writer.append(make_event(&ws, i)).await.unwrap();
            let captured = sink.events.lock();
            let event = captured.last().unwrap();
            assert_eq!(event.prev_hash, last);
            assert_eq!(event.event_hash, h);
            last = h;
        }
    }

    #[tokio::test]
    async fn batch_is_emitted_on_threshold() {
        let secret = SigningSecret::generate();
        let mut writer = AuditWriter::new(
            secret,
            WriterConfig { batch_size: 4 },
        );
        let sink = CapturingSink::new();
        writer.add_sink(sink.clone());
        let ws = WorkspaceId::new("acme").unwrap();
        for i in 0..4 {
            writer.append(make_event(&ws, i)).await.unwrap();
        }
        assert_eq!(sink.batches.lock().len(), 1);
        let batch = sink.batches.lock()[0].clone();
        assert_eq!(batch.leaf_count, 4);
    }

    #[tokio::test]
    async fn manual_flush_drains_buffer() {
        let secret = SigningSecret::generate();
        let mut writer = AuditWriter::new(
            secret,
            WriterConfig { batch_size: 1024 },
        );
        let sink = CapturingSink::new();
        writer.add_sink(sink.clone());
        let ws = WorkspaceId::new("acme").unwrap();
        for i in 0..3 {
            writer.append(make_event(&ws, i)).await.unwrap();
        }
        let batch = writer.flush(&ws).await.unwrap().expect("batch");
        assert_eq!(batch.leaf_count, 3);
        // A second flush on empty pending buffer returns None.
        assert!(writer.flush(&ws).await.unwrap().is_none());
    }
}
