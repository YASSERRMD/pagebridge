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

pub mod markdown;
pub mod pdf;
pub mod plain;
pub mod tree;
pub mod worker;

pub use worker::{SummaryTask, SummaryWorkerConfig};

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
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
    let start = now_ms();
    let built = build_structural(&params)?;
    let root_node_id = NodeId::root(&built.doc_id);
    let doc_id = built.doc_id.clone();

    storage.upsert_nodes(&built.nodes).await?;
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

    // Background summary task.
    let storage2 = Arc::clone(&storage);
    let llm2 = Arc::clone(&llm);
    let prompts2 = Arc::clone(&prompts);
    let nodes = built.nodes;
    let join = tokio::spawn(async move { run_summaries(storage2, llm2, prompts2, nodes).await });

    Ok((handle, join))
}

async fn run_summaries(
    storage: Arc<dyn StorageAdapter>,
    llm: Arc<dyn LlmProvider>,
    prompts: Arc<PromptLibrary>,
    nodes: Vec<NodeRecord>,
) -> Result<()> {
    let mut ordered = nodes;
    // Deepest first so when we summarize a parent its children already have
    // routing summaries we can include.
    ordered.sort_by(|a, b| b.node_id.depth().cmp(&a.node_id.depth()));
    let fingerprint = format!("{}:{}", llm.name(), llm.model());

    for n in &ordered {
        if n.is_leaf {
            // Leaf summaries are the body text trimmed to a useful preview; no LLM needed.
            let mut updated = n.clone();
            if updated.summary.is_empty() {
                updated.summary = updated.routing_summary.clone();
            }
            storage.upsert_node(&updated).await?;
            continue;
        }

        let children = storage.children_records(&n.node_id).await?;
        if children.is_empty() {
            continue;
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

        // Cache lookup keyed by the payload hash + model fingerprint.
        let mut cached: Option<SummaryCacheEntry> = storage
            .get_summary_cache(&payload_hash)
            .await?
            .filter(|e| e.model_fingerprint == fingerprint);

        if cached.is_none() {
            let ctx = PromptContext {
                document_title: Some(n.title.clone()),
                raw_text: Some(child_payload.clone()),
                ..PromptContext::default()
            };
            let prompt = prompts.render("summarize", &ctx)?;
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
                model_fingerprint: fingerprint.clone(),
                created_at: now_ms(),
            };
            storage.upsert_summary_cache(&payload_hash, &entry).await?;
            cached = Some(entry);
        }

        let entry = cached.expect("cache populated above");
        let mut updated = n.clone();
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
