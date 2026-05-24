//! Background writer that drains updated node records into batched storage
//! upserts.
//!
//! The summary fan-out workers (see [`super::summarize_document_parallel`])
//! send fully-updated [`NodeRecord`]s into a channel; the [`BatchWriter`]
//! coalesces them up to `batch_size` per flush or every `flush_interval`,
//! whichever comes first. This collapses N per-node transactions into N/B
//! batched ones with the corresponding throughput multiplier.

use std::sync::Arc;
use std::time::Duration;

use crate::adapter::StorageAdapter;
use crate::error::Result;
use crate::record::NodeRecord;

/// Outcome of the writer's flush loop.
#[derive(Debug, Clone, Copy, Default)]
pub struct WriterStats {
    pub batches: u64,
    pub records: u64,
    pub failures: u64,
}

impl WriterStats {
    /// True when at least one record failed even after the retry budget was
    /// spent. Callers should treat the document as `partial_ingest` and
    /// surface that to operators.
    #[must_use]
    pub const fn is_partial(&self) -> bool {
        self.failures > 0
    }
}

/// Tunable knobs for the batch writer.
#[derive(Debug, Clone, Copy)]
pub struct BatchWriterConfig {
    pub batch_size: usize,
    pub flush_interval: Duration,
    pub max_retries: u32,
}

impl Default for BatchWriterConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval: Duration::from_millis(200),
            max_retries: 3,
        }
    }
}

/// Spawn a background drainer that batches `records_rx` into
/// [`StorageAdapter::upsert_nodes_batch`] calls. Returns a join handle whose
/// `Ok(WriterStats)` reports how many batches and records were written.
pub fn spawn(
    storage: Arc<dyn StorageAdapter>,
    records_rx: async_channel::Receiver<NodeRecord>,
    config: BatchWriterConfig,
) -> tokio::task::JoinHandle<Result<WriterStats>> {
    let effective_batch = config.batch_size.max(storage.recommended_batch_size() / 2);
    tokio::spawn(async move {
        let mut buf: Vec<NodeRecord> = Vec::with_capacity(effective_batch);
        let mut stats = WriterStats::default();
        loop {
            tokio::select! {
                rec = records_rx.recv() => {
                    match rec {
                        Ok(r) => {
                            buf.push(r);
                            if buf.len() >= effective_batch {
                                flush(&storage, &mut buf, &mut stats, config.max_retries).await;
                            }
                        }
                        Err(_) => {
                            // Channel closed; flush remainder and exit.
                            flush(&storage, &mut buf, &mut stats, config.max_retries).await;
                            return Ok(stats);
                        }
                    }
                }
                _ = tokio::time::sleep(config.flush_interval) => {
                    flush(&storage, &mut buf, &mut stats, config.max_retries).await;
                }
            }
        }
    })
}

async fn flush(
    storage: &Arc<dyn StorageAdapter>,
    buf: &mut Vec<NodeRecord>,
    stats: &mut WriterStats,
    max_retries: u32,
) {
    if buf.is_empty() {
        return;
    }
    let mut attempt = 0;
    let mut backoff_ms = 50u64;
    loop {
        match storage.upsert_nodes_batch(buf).await {
            Ok(()) => {
                stats.batches += 1;
                stats.records += buf.len() as u64;
                buf.clear();
                return;
            }
            Err(e) if attempt < max_retries => {
                tracing::warn!(error = %e, attempt, "batch write failed; retrying");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = backoff_ms.saturating_mul(2);
                attempt += 1;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    batch_size = buf.len(),
                    "batch write failed permanently; dropping batch"
                );
                stats.failures += buf.len() as u64;
                buf.clear();
                return;
            }
        }
    }
}
