//! Ingestion pipeline.
//!
//! Turns raw bytes (markdown, plain text, or PDF) into a hierarchical tree of
//! [`crate::record::NodeRecord`]s, persists the structural tree, then in a
//! background task fills in summaries via the configured LLM provider.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::option_if_let_else,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::module_name_repetitions,
    clippy::redundant_clone,
    clippy::map_unwrap_or,
    clippy::type_complexity,
    clippy::explicit_iter_loop,
    clippy::needless_lifetimes,
    clippy::doc_markdown,
    clippy::assigning_clones,
    clippy::format_push_string,
    clippy::format_collect,
    clippy::single_match_else,
    clippy::if_not_else,
    clippy::manual_let_else,
    clippy::elidable_lifetime_names,
    clippy::needless_borrows_for_generic_args,
    clippy::doc_overindented_list_items,
    clippy::ignored_unit_patterns,
    clippy::too_long_first_doc_paragraph,
    clippy::unused_self
)]

pub mod batch_writer;
pub mod markdown;
pub mod pdf;
pub mod plain;
pub mod progress;
pub mod scheduler;
pub mod tree;
pub mod worker;

pub use batch_writer::{BatchWriterConfig, WriterStats};
pub use progress::{IngestStage, ProgressSnapshot, ProgressTracker};
pub use scheduler::DispatchScheduler;
pub use worker::{SummaryTask, SummaryWorkerConfig};

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::adapter::StorageAdapter;
use crate::error::{PagebridgeError, Result};
use crate::id::{DocId, NodeId};
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider};
use crate::prompts::{PromptContext, PromptLibrary};
use crate::record::{NodeLevel, NodeRecord};
use crate::types::{DocumentEntry, DocumentHandle, IngestParams, SourceKind, SummaryCacheEntry};

/// Build a tree from raw bytes per the given source kind. Returns
/// `(doc_id, leaves_count, byte_count, raw_text, nodes)`.
pub fn build_structural(params: &IngestParams) -> Result<BuildResult> {
    let doc_id = params
        .doc_id
        .clone()
        .map_or_else(|| auto_doc_id(&params.title), Ok)?;
    let raw_text = std::str::from_utf8(&params.raw_text).map_err(|e| PagebridgeError::Parse {
        source_kind: format!("{:?}", params.source_kind).to_lowercase(),
        message: format!("non-utf8 input: {e}"),
    })?;
    let nodes = match params.source_kind {
        SourceKind::Markdown => markdown::parse(&doc_id, &params.title, raw_text)?,
        SourceKind::Plain => plain::parse(&doc_id, &params.title, raw_text)?,
        SourceKind::Pdf => pdf::parse_bytes(&doc_id, &params.title, &params.raw_text)?,
    };
    let leaves = nodes.iter().filter(|n| n.is_leaf).count() as u32;
    Ok(BuildResult {
        doc_id,
        leaf_count: leaves,
        byte_count: params.raw_text.len() as u64,
        nodes,
    })
}

/// Result of structural tree construction.
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub doc_id: DocId,
    pub leaf_count: u32,
    pub byte_count: u64,
    pub nodes: Vec<NodeRecord>,
}

/// Drive a full ingestion against the given adapter, spawning a background
/// task for summary work. Returns the structural handle plus the JoinHandle
/// so callers can await summary completion via `wait_for_summaries`.
pub async fn ingest(
    storage: Arc<dyn StorageAdapter>,
    llm: Arc<dyn LlmProvider>,
    prompts: Arc<PromptLibrary>,
    params: IngestParams,
) -> Result<(DocumentHandle, JoinHandle<Result<()>>)> {
    ingest_with_config(storage, llm, prompts, params, SummaryWorkerConfig::default()).await
}

/// Drive a full ingestion using the specified [`SummaryWorkerConfig`]. The
/// `SummaryWorkerConfig::default()` variant matches the historical sequential
/// behavior when `max_concurrency = 1`.
pub async fn ingest_with_config(
    storage: Arc<dyn StorageAdapter>,
    llm: Arc<dyn LlmProvider>,
    prompts: Arc<PromptLibrary>,
    params: IngestParams,
    worker_config: SummaryWorkerConfig,
) -> Result<(DocumentHandle, JoinHandle<Result<()>>)> {
    let start = now_ms();
    let built = build_structural(&params)?;
    let root_node_id = NodeId::root(&built.doc_id);
    let doc_id = built.doc_id.clone();

    // Use the explicit batch API so storage backends collapse 250 inserts
    // into one transaction (50-100x faster on SQLite/Postgres).
    storage.upsert_nodes_batch(&built.nodes).await?;
    storage.put_raw(&doc_id, &params.raw_text).await?;
    storage
        .upsert_document(&DocumentEntry {
            doc_id: doc_id.clone(),
            title: params.title.clone(),
            source_kind: params.source_kind.as_str().to_owned(),
            ingested_at: start,
            root_node_id: root_node_id.clone(),
            leaf_count: built.leaf_count,
            byte_count: built.byte_count,
        })
        .await?;

    let handle = DocumentHandle {
        doc_id: doc_id.clone(),
        root_node_id,
        leaf_count: built.leaf_count,
        byte_count: built.byte_count,
        structural_ingest_ms: now_ms().saturating_sub(start) as u64,
    };

    let storage2 = Arc::clone(&storage);
    let llm2 = Arc::clone(&llm);
    let prompts2 = Arc::clone(&prompts);
    let nodes = built.nodes;
    let join = tokio::spawn(async move {
        summarize_document_parallel(storage2, llm2, prompts2, nodes, worker_config).await
    });

    Ok((handle, join))
}

/// Drive summary work for every node in `nodes`, level-by-level, with up to
/// [`SummaryWorkerConfig::max_concurrency`] tasks in flight per level.
///
/// Bottom-up ordering is preserved: leaves first (no LLM), then sections,
/// then chapters, then root. Within a level, every task runs in parallel,
/// bounded by a semaphore. The next level does not start until the current
/// level fully drains, which guarantees that when a parent is summarized
/// every child already has its summary persisted.
async fn summarize_document_parallel(
    storage: Arc<dyn StorageAdapter>,
    llm: Arc<dyn LlmProvider>,
    prompts: Arc<PromptLibrary>,
    nodes: Vec<NodeRecord>,
    config: SummaryWorkerConfig,
) -> Result<()> {
    let fingerprint = format!("{}:{}", llm.name(), llm.model());

    // Drain summary updates through a BatchWriter so per-node upserts become
    // per-batch transactions. Channel capacity is generous; the writer keeps
    // up with even high-concurrency fan-outs because LLM latency dominates.
    let (writer_tx, writer_rx) = async_channel::bounded::<NodeRecord>(4096);
    let writer_cfg = BatchWriterConfig {
        batch_size: storage.recommended_batch_size().clamp(50, 1024),
        ..BatchWriterConfig::default()
    };
    let writer_join = batch_writer::spawn(Arc::clone(&storage), writer_rx, writer_cfg);

    let by_depth = group_nodes_by_depth(nodes);

    // Consult the provider's declared rate limits so the scheduler does not
    // exceed the provider-side concurrent-request cap and so RPM/TPM permits
    // are awaited before each dispatch.
    let dispatch = Arc::new(DispatchScheduler::from_limits(
        llm.rate_limits(),
        config.max_concurrency,
    ));
    let effective_concurrency = dispatch.effective_concurrency as usize;
    let semaphore = Arc::new(Semaphore::new(effective_concurrency));
    tracing::debug!(
        max_concurrency = effective_concurrency,
        rpm = ?llm.rate_limits().requests_per_minute,
        levels = by_depth.len(),
        "starting parallel summary fan-out"
    );

    // Process from deepest depth (highest key) down to root (depth 0). The
    // outer `for` loop is the level barrier: every task at depth N+1 finishes
    // before any task at depth N starts. This invariant is what guarantees
    // a parent sees fully persisted child summaries.
    for (depth, level_nodes) in by_depth.into_iter().rev() {
        let level_size = level_nodes.len();
        tracing::trace!(depth, level_size, "summarizing level");
        let mut handles = Vec::with_capacity(level_size);
        for node in level_nodes {
            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                break;
            };
            // Await an RPM permit before scheduling the LLM call. Tokens are
            // awaited inside summarize_one_node once we know the prompt size.
            dispatch.await_request_permit().await;
            let storage = Arc::clone(&storage);
            let llm = Arc::clone(&llm);
            let prompts = Arc::clone(&prompts);
            let fingerprint = fingerprint.clone();
            let dispatch_ref = Arc::clone(&dispatch);
            let writer_tx = writer_tx.clone();
            let task_timeout = std::time::Duration::from_millis(config.timeout_per_task_ms);
            let handle = tokio::spawn(async move {
                let _permit = permit;
                let fut = summarize_one_node(
                    &storage,
                    &llm,
                    &prompts,
                    &node,
                    &fingerprint,
                    Some(&dispatch_ref),
                    Some(&writer_tx),
                );
                match tokio::time::timeout(task_timeout, fut).await {
                    Ok(r) => r,
                    Err(_) => {
                        // Per-task timeout fired. Note as a soft throttle so
                        // the scheduler backs off if these repeat.
                        dispatch_ref.note_throttle();
                        tracing::warn!(
                            timeout_ms = task_timeout.as_millis() as u64,
                            "summary task per-call timeout exceeded"
                        );
                        Ok(())
                    }
                }
            });
            handles.push(handle);
        }
        // Drain the whole level. Per-task errors are logged but do not abort
        // the document: missing one summary is recoverable, aborting is not.
        for h in handles {
            match h.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "summary task failed"),
                Err(join_err) => tracing::warn!(error = %join_err, "summary task panicked"),
            }
        }
    }

    // Close the writer side; the BatchWriter flushes its remainder and exits.
    drop(writer_tx);
    match writer_join.await {
        Ok(Ok(stats)) => {
            tracing::debug!(
                batches = stats.batches,
                records = stats.records,
                failures = stats.failures,
                "BatchWriter drained"
            );
            if stats.failures > 0 {
                tracing::warn!(
                    failures = stats.failures,
                    "ingest finished with {} dropped summary records; treat document as partial_ingest",
                    stats.failures
                );
            }
        }
        Ok(Err(e)) => tracing::warn!(error = %e, "BatchWriter exited with error"),
        Err(e) => tracing::warn!(error = %e, "BatchWriter join failed"),
    }
    // Force a write-side flush so search sees every summary immediately. On
    // SQL adapters this is a no-op; on the embedded adapter it commits the
    // tantivy writer past any deferred segment writes.
    if let Err(e) = storage.flush().await {
        tracing::warn!(error = %e, "post-summary flush failed");
    }
    Ok(())
}

/// Group nodes by tree depth. Returned map is keyed by `node_id.depth()`,
/// with deeper levels at higher keys. Useful for callers that want to drive
/// their own per-level pipeline.
#[must_use]
pub fn group_nodes_by_depth(nodes: Vec<NodeRecord>) -> BTreeMap<usize, Vec<NodeRecord>> {
    let mut by_depth: BTreeMap<usize, Vec<NodeRecord>> = BTreeMap::new();
    for n in nodes {
        by_depth.entry(n.node_id.depth()).or_default().push(n);
    }
    by_depth
}

async fn summarize_one_node(
    storage: &Arc<dyn StorageAdapter>,
    llm: &Arc<dyn LlmProvider>,
    prompts: &Arc<PromptLibrary>,
    node: &NodeRecord,
    fingerprint: &str,
    dispatch: Option<&Arc<DispatchScheduler>>,
    writer_tx: Option<&async_channel::Sender<NodeRecord>>,
) -> Result<()> {
    if node.is_leaf {
        let mut updated = node.clone();
        if updated.summary.is_empty() {
            updated.summary = updated.routing_summary.clone();
        }
        write_updated(storage, writer_tx, updated).await?;
        return Ok(());
    }

    let children = storage.children_records(&node.node_id).await?;
    if children.is_empty() {
        return Ok(());
    }
    let mut child_payload = String::new();
    for c in &children {
        child_payload.push_str(&format!(
            "## {} ({})\n{}\n\n",
            c.title,
            c.node_id.as_str(),
            if c.routing_summary.is_empty() {
                &c.summary
            } else {
                &c.routing_summary
            }
        ));
    }
    let payload_hash = source_hash(child_payload.as_bytes());

    let mut cached: Option<SummaryCacheEntry> = storage
        .get_summary_cache(&payload_hash)
        .await?
        .filter(|e| e.model_fingerprint == fingerprint);

    if cached.is_none() {
        let ctx = PromptContext {
            document_title: Some(node.title.clone()),
            raw_text: Some(child_payload.clone()),
            ..PromptContext::default()
        };
        let prompt = prompts.render("summarize", &ctx)?;
        if let Some(sched) = dispatch {
            // Pay tokens-per-minute before issuing the request. The estimate
            // is a lower bound but the limiter naturally backpressures, so a
            // slight overshoot only costs a few milliseconds of wait.
            let estimated = u32::try_from(llm.estimate_tokens(&prompt)).unwrap_or(u32::MAX);
            sched.await_token_permits(estimated).await;
        }
        let resp = llm
            .complete_json(
                CompletionRequest {
                    system: Some("You are a precise summarizer.".into()),
                    messages: vec![ChatMessage::user(prompt)],
                    ..CompletionRequest::default()
                },
                &PromptLibrary::summarize_schema(),
            )
            .await?;
        let routing_summary = resp
            .get("routing_summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let summary = resp
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let keywords: Vec<String> = resp
            .get("keywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let entry = SummaryCacheEntry {
            routing_summary,
            summary,
            keywords,
            model_fingerprint: fingerprint.to_owned(),
            created_at: now_ms(),
        };
        storage.upsert_summary_cache(&payload_hash, &entry).await?;
        cached = Some(entry);
    }

    let entry = cached.expect("cache populated above");
    let mut updated = node.clone();
    updated.routing_summary = if entry.routing_summary.is_empty() {
        updated.routing_summary
    } else {
        entry.routing_summary
    };
    if updated.summary.is_empty() || updated.summary == updated.routing_summary {
        updated.summary = entry.summary;
    }
    if updated.keywords.is_empty() {
        updated.keywords = entry.keywords;
    }
    updated.updated_at = now_ms();
    write_updated(storage, writer_tx, updated).await?;
    Ok(())
}

/// Route an updated node either through the batch writer channel (if
/// present) or straight to the adapter as a fallback.
async fn write_updated(
    storage: &Arc<dyn StorageAdapter>,
    writer_tx: Option<&async_channel::Sender<NodeRecord>>,
    updated: NodeRecord,
) -> Result<()> {
    if let Some(tx) = writer_tx {
        // Channel send only fails when the receiver is dropped, which means
        // the BatchWriter is shutting down; fall back to a direct upsert so
        // the work is not lost.
        if let Err(send_err) = tx.send(updated.clone()).await {
            tracing::debug!(error = %send_err, "BatchWriter closed; direct upsert");
            storage.upsert_node(&updated).await?;
        }
    } else {
        storage.upsert_node(&updated).await?;
    }
    Ok(())
}

/// SHA-256 of `bytes`.
#[must_use]
pub fn source_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn auto_doc_id(title: &str) -> Result<DocId> {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(now_ms().to_be_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let slug = slugify_title(title);
    let combined = if slug.is_empty() {
        format!("doc-{hex}")
    } else {
        let trimmed: String = slug.chars().take(40).collect();
        format!("{trimmed}-{hex}")
    };
    DocId::new(combined)
}

fn slugify_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Convenience: build a leaf NodeRecord.
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_leaf(
    doc_id: &DocId,
    parent_id: &NodeId,
    leaf_seq: u32,
    title: String,
    body_span: (u64, u64),
    page_start: Option<u32>,
    page_end: Option<u32>,
    routing_summary: String,
) -> Result<NodeRecord> {
    let node_id = parent_id.child("leaf", &leaf_seq.to_string())?;
    Ok(NodeRecord {
        node_id,
        doc_id: doc_id.clone(),
        parent_id: Some(parent_id.clone()),
        title,
        level: NodeLevel::Leaf,
        routing_summary,
        summary: String::new(),
        child_ids: vec![],
        span: Some(body_span),
        page_start,
        page_end,
        keywords: vec![],
        is_leaf: true,
        created_at: now_ms(),
        updated_at: now_ms(),
        source_hash: [0; 32],
    })
}
