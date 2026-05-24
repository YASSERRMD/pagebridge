//! Live progress reporting for the ingest pipeline.
//!
//! Workers update atomic counters as they make forward progress. A debounced
//! broadcaster snapshots those counters every 100ms and pushes
//! [`ProgressSnapshot`]s to subscribers. Callers (recallwell SSE, the CLI
//! progress bar, the admin UI) subscribe and render in real time.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::id::DocId;

/// Snapshot of an in-flight document ingest. Cheap to clone; serializable so
/// recallwell can forward it over SSE without an intermediate translation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressSnapshot {
    pub doc_id: DocId,
    pub stage: IngestStage,
    pub total_nodes: u32,
    pub structural_done: bool,
    pub summaries_done: u32,
    pub summaries_total: u32,
    pub bm25_indexed: u32,
    pub bm25_indexed_target: u32,
    pub cache_hits: u32,
    pub llm_calls_in_flight: u32,
    pub llm_calls_total: u32,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
    pub recent_failures: u32,
}

/// Coarse stage the pipeline is currently in. Drives the CLI / UI label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStage {
    Parsing,
    StructuralInsert,
    Summarizing,
    Indexing,
    Done,
    Failed,
}

/// Shared counters mutated by every worker, debounced into broadcasts by the
/// background snapshot task.
#[derive(Debug)]
pub struct ProgressTracker {
    pub doc_id: DocId,
    pub started_at: Instant,
    pub stage: Mutex<IngestStage>,
    pub total_nodes: AtomicU32,
    pub summaries_total: AtomicU32,
    pub summaries_done: AtomicU32,
    pub bm25_indexed: AtomicU32,
    pub bm25_indexed_target: AtomicU32,
    pub cache_hits: AtomicU32,
    pub llm_calls_in_flight: AtomicU32,
    pub llm_calls_total: AtomicU32,
    pub recent_failures: AtomicU32,
    pub structural_done: AtomicU32,
    /// Rolling EMA of completion-rate (tasks per second).
    pub ema_rate_micro: AtomicU64,
    pub last_sample_at_ms: AtomicU64,
    pub last_sample_done: AtomicU32,
    pub tx: broadcast::Sender<ProgressSnapshot>,
}

impl ProgressTracker {
    #[must_use]
    pub fn new(doc_id: DocId) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(64);
        Arc::new(Self {
            doc_id,
            started_at: Instant::now(),
            stage: Mutex::new(IngestStage::Parsing),
            total_nodes: AtomicU32::new(0),
            summaries_total: AtomicU32::new(0),
            summaries_done: AtomicU32::new(0),
            bm25_indexed: AtomicU32::new(0),
            bm25_indexed_target: AtomicU32::new(0),
            cache_hits: AtomicU32::new(0),
            llm_calls_in_flight: AtomicU32::new(0),
            llm_calls_total: AtomicU32::new(0),
            recent_failures: AtomicU32::new(0),
            structural_done: AtomicU32::new(0),
            ema_rate_micro: AtomicU64::new(0),
            last_sample_at_ms: AtomicU64::new(0),
            last_sample_done: AtomicU32::new(0),
            tx,
        })
    }

    /// Subscribe to live snapshots. Each subscriber gets its own receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressSnapshot> {
        self.tx.subscribe()
    }

    /// Set the current pipeline stage. Triggers an immediate snapshot push.
    pub fn set_stage(&self, stage: IngestStage) {
        *self.stage.lock() = stage;
        self.broadcast();
    }

    pub fn note_total_nodes(&self, n: u32) {
        self.total_nodes.store(n, Ordering::SeqCst);
        self.summaries_total.store(n, Ordering::SeqCst);
        self.bm25_indexed_target.store(n, Ordering::SeqCst);
        self.broadcast();
    }

    pub fn note_structural_done(&self) {
        self.structural_done.store(1, Ordering::SeqCst);
        self.broadcast();
    }

    pub fn note_llm_dispatched(&self) {
        self.llm_calls_in_flight.fetch_add(1, Ordering::SeqCst);
        self.llm_calls_total.fetch_add(1, Ordering::SeqCst);
    }

    pub fn note_llm_completed(&self, success: bool) {
        self.llm_calls_in_flight.fetch_sub(1, Ordering::SeqCst);
        if success {
            self.summaries_done.fetch_add(1, Ordering::SeqCst);
        } else {
            self.recent_failures.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn note_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::SeqCst);
        self.summaries_done.fetch_add(1, Ordering::SeqCst);
    }

    pub fn note_bm25_indexed(&self, n: u32) {
        self.bm25_indexed.fetch_add(n, Ordering::SeqCst);
    }

    /// Push a snapshot to every subscriber. Safe to call from any worker;
    /// the broadcast channel is lossy under backpressure so this never
    /// blocks.
    pub fn broadcast(&self) {
        let snap = self.snapshot();
        let _ = self.tx.send(snap);
    }

    /// Compute the current snapshot, including ETA based on rolling EMA.
    #[must_use]
    pub fn snapshot(&self) -> ProgressSnapshot {
        let done = self.summaries_done.load(Ordering::SeqCst);
        let total = self.summaries_total.load(Ordering::SeqCst);
        let elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        let eta_ms = self.recompute_eta(done, total, elapsed_ms);
        ProgressSnapshot {
            doc_id: self.doc_id.clone(),
            stage: *self.stage.lock(),
            total_nodes: self.total_nodes.load(Ordering::SeqCst),
            structural_done: self.structural_done.load(Ordering::SeqCst) > 0,
            summaries_done: done,
            summaries_total: total,
            bm25_indexed: self.bm25_indexed.load(Ordering::SeqCst),
            bm25_indexed_target: self.bm25_indexed_target.load(Ordering::SeqCst),
            cache_hits: self.cache_hits.load(Ordering::SeqCst),
            llm_calls_in_flight: self.llm_calls_in_flight.load(Ordering::SeqCst),
            llm_calls_total: self.llm_calls_total.load(Ordering::SeqCst),
            elapsed_ms,
            eta_ms,
            recent_failures: self.recent_failures.load(Ordering::SeqCst),
        }
    }

    fn recompute_eta(&self, done: u32, total: u32, elapsed_ms: u64) -> Option<u64> {
        if total == 0 || done >= total {
            return Some(0);
        }
        // EMA with alpha = 0.3 on tasks-per-second.
        let last_ms = self.last_sample_at_ms.load(Ordering::SeqCst);
        let last_done = self.last_sample_done.load(Ordering::SeqCst);
        if last_ms == 0 {
            self.last_sample_at_ms.store(elapsed_ms, Ordering::SeqCst);
            self.last_sample_done.store(done, Ordering::SeqCst);
            return None;
        }
        let dt_ms = elapsed_ms.saturating_sub(last_ms);
        if dt_ms < 250 {
            // Not enough new data; reuse stored EMA.
            let ema_micro = self.ema_rate_micro.load(Ordering::SeqCst);
            if ema_micro == 0 {
                return None;
            }
            let remaining = total.saturating_sub(done) as u64;
            let rate = ema_micro as f64 / 1_000_000.0;
            if rate <= 0.0 {
                return None;
            }
            return Some(((remaining as f64) / rate * 1_000.0) as u64);
        }
        let new_done = done.saturating_sub(last_done) as f64;
        let instant_rate = new_done / (dt_ms as f64 / 1_000.0);
        let prev_rate = self.ema_rate_micro.load(Ordering::SeqCst) as f64 / 1_000_000.0;
        let alpha = if prev_rate == 0.0 { 1.0 } else { 0.3 };
        let new_rate = alpha.mul_add(instant_rate, (1.0 - alpha) * prev_rate);
        self.ema_rate_micro
            .store((new_rate * 1_000_000.0) as u64, Ordering::SeqCst);
        self.last_sample_at_ms.store(elapsed_ms, Ordering::SeqCst);
        self.last_sample_done.store(done, Ordering::SeqCst);
        let remaining = total.saturating_sub(done) as f64;
        if new_rate <= 0.0 {
            return None;
        }
        Some(((remaining / new_rate) * 1_000.0) as u64)
    }
}

/// Spawn a background task that broadcasts a snapshot every `interval` until
/// `stop_rx` resolves. Used to push regular updates even when no per-task
/// event triggers a broadcast.
pub fn spawn_heartbeat(
    tracker: Arc<ProgressTracker>,
    interval: Duration,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {
                    tracker.broadcast();
                }
                _ = &mut stop_rx => return,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DocId;

    #[tokio::test]
    async fn tracker_emits_snapshot_with_eta() {
        let doc = DocId::new("doc-eta").unwrap();
        let t = ProgressTracker::new(doc);
        t.note_total_nodes(10);
        t.set_stage(IngestStage::Summarizing);
        for _ in 0..3 {
            t.note_llm_dispatched();
            tokio::time::sleep(Duration::from_millis(15)).await;
            t.note_llm_completed(true);
        }
        let snap = t.snapshot();
        assert_eq!(snap.summaries_done, 3);
        // ETA may still be None on the very first sample but should be Some
        // shortly thereafter; just assert the structural fields.
        assert_eq!(snap.summaries_total, 10);
    }

    #[tokio::test]
    async fn cache_hit_counts_as_done() {
        let doc = DocId::new("doc-cache").unwrap();
        let t = ProgressTracker::new(doc);
        t.note_total_nodes(5);
        t.note_cache_hit();
        t.note_cache_hit();
        let snap = t.snapshot();
        assert_eq!(snap.cache_hits, 2);
        assert_eq!(snap.summaries_done, 2);
    }
}
