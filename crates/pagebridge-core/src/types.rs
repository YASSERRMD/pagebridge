//! Supporting types: ingest params, document entries, search hits, answers, traces.

use std::collections::BTreeMap;

use crate::id::{DocId, NodeId};

/// What kind of source a document was ingested from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Markdown,
    Plain,
    Pdf,
}

impl SourceKind {
    /// Returns a stable lowercase tag.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Plain => "plain",
            Self::Pdf => "pdf",
        }
    }
}

/// Parameters for ingesting a new document.
#[derive(Debug, Clone)]
pub struct IngestParams {
    pub title: String,
    pub source_kind: SourceKind,
    pub raw_text: Vec<u8>,
    /// Auto-generated from title + timestamp if not provided.
    pub doc_id: Option<DocId>,
    pub user_metadata: BTreeMap<String, String>,
}

/// How `update_document` should reconcile new content with the stored version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    /// Full re-ingest. Old nodes are removed, new ones inserted. The summary
    /// cache (hash-keyed) lets unchanged leaves reuse summaries for free.
    Replace,
    /// Diff old leaves against new ones by content hash; reuse unchanged
    /// nodes verbatim, regenerate only the changed ancestors. v0.5 ships
    /// this as a stub that falls back to `Replace`; v0.6 will land the real
    /// chunk-diff algorithm.
    Incremental,
    /// Append-only fast path for log-like documents: only new content past
    /// the existing end-offset is processed.
    AppendOnly,
}

/// Parameters for updating an existing document.
#[derive(Debug, Clone)]
pub struct UpdateParams {
    pub doc_id: DocId,
    pub new_raw_text: Vec<u8>,
    pub diff_mode: DiffMode,
}

/// Report from `update_document`, mirroring `DocumentHandle` plus diff metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateReport {
    pub doc_id: DocId,
    pub root_node_id: NodeId,
    pub leaf_count: u32,
    pub byte_count: u64,
    pub unchanged_leaves: u32,
    pub changed_leaves: u32,
    pub new_leaves: u32,
    pub removed_leaves: u32,
    pub structural_ingest_ms: u64,
}

/// Handle returned from `ingest_document`. The structural insert is complete by the
/// time you hold this; background summary work can be awaited via `wait_for_summaries`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentHandle {
    pub doc_id: DocId,
    pub root_node_id: NodeId,
    pub leaf_count: u32,
    pub byte_count: u64,
    pub structural_ingest_ms: u64,
}

/// Lightweight handle returned by [`crate::Pagebridge::ingest_document_with_progress`].
///
/// Holds the same identifying fields as [`DocumentHandle`] plus a live
/// progress channel callers can subscribe to. `.wait()` resolves to the
/// completed [`DocumentHandle`] once the background summary task drains.
#[derive(Debug)]
pub struct DocumentIngestHandle {
    pub doc_id: DocId,
    pub root_node_id: NodeId,
    pub leaf_count: u32,
    pub byte_count: u64,
    pub structural_ingest_ms: u64,
    pub(crate) progress: std::sync::Arc<crate::ingest::ProgressTracker>,
    pub(crate) join: tokio::task::JoinHandle<crate::error::Result<()>>,
}

impl DocumentIngestHandle {
    /// Snapshot of the current progress state.
    #[must_use]
    pub fn progress(&self) -> crate::ingest::ProgressSnapshot {
        self.progress.snapshot()
    }

    /// Subscribe to a live stream of progress updates. Each subscriber gets
    /// its own receiver; updates are debounced to ~100ms per snapshot.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<crate::ingest::ProgressSnapshot> {
        self.progress.subscribe()
    }

    /// Identifier for the in-flight document.
    #[must_use]
    pub fn doc_id(&self) -> &DocId {
        &self.doc_id
    }

    /// Await full ingest completion. The returned [`DocumentHandle`] is
    /// equivalent to what the legacy `ingest_document` produced once the
    /// background summary task finished.
    pub async fn wait(self) -> crate::error::Result<DocumentHandle> {
        self.join
            .await
            .map_err(|e| crate::error::PagebridgeError::Internal(format!("join: {e}")))??;
        Ok(DocumentHandle {
            doc_id: self.doc_id,
            root_node_id: self.root_node_id,
            leaf_count: self.leaf_count,
            byte_count: self.byte_count,
            structural_ingest_ms: self.structural_ingest_ms,
        })
    }
}

/// Cached summary entry keyed by the sha256 of the source content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SummaryCacheEntry {
    pub routing_summary: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub model_fingerprint: String,
    pub created_at: i64,
}

/// One row in the document index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocumentEntry {
    pub doc_id: DocId,
    pub title: String,
    pub source_kind: String,
    pub ingested_at: i64,
    pub root_node_id: NodeId,
    pub leaf_count: u32,
    pub byte_count: u64,
    /// Sha256 over the document's raw text. `None` for entries written by
    /// pre-Phase-I8 ingests. Populated by [`crate::ingest`] so re-ingest
    /// fast-paths can compare without walking the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_text_hash: Option<[u8; 32]>,
    /// Sha256 over the structural skeleton (titles + levels + parent edges)
    /// for the document. `None` for legacy entries; populated by Phase I8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_hash: Option<[u8; 32]>,
}

/// Prediction returned by [`crate::Pagebridge::would_reingest_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReingestPrediction {
    /// Both raw-text and structural hash match the stored document; re-ingest
    /// will be a no-op.
    NoChange,
    /// Structural hash differs but raw-text hash matches (rare: parser
    /// version bump, etc).
    StructuralOnly { changed_node_count: u32 },
    /// Raw text differs but leaf-level diff shows most content unchanged.
    PartialContent {
        changed_leaf_count: u32,
        total_leaves: u32,
    },
    /// Document is unknown or differs entirely; full re-ingest will run.
    FullChange,
}

/// One hit from a BM25 search. Scores are always "higher is more relevant".
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchHit {
    pub node_id: NodeId,
    pub doc_id: DocId,
    pub title: String,
    pub score: f32,
}

/// Citation produced during synthesis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Citation {
    pub node_id: NodeId,
    pub doc_id: DocId,
    pub doc_title: String,
    pub section_title: String,
    pub page_range: Option<(u32, u32)>,
    pub excerpt: String,
}

/// A chunk produced by [`crate::Pagebridge::ask_stream`]. Concatenating every
/// `Token` event yields the same text the non-streaming `ask` would produce.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerChunk {
    /// A piece of generated text.
    Token { text: String },
    /// A resolved citation emitted as the synthesizer references a leaf.
    Citation { citation: Citation },
    /// Terminal chunk with the full query trace and consolidated citation list.
    Done {
        trace: QueryTrace,
        citations: Vec<Citation>,
    },
}

/// A single step in a query trace. See [`QueryTrace`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceStep {
    Bm25Candidates {
        count: usize,
        top_score: f32,
    },
    NavigationDecision {
        node_id: NodeId,
        action: String,
        reason: Option<String>,
        input_tokens: u32,
        output_tokens: u32,
        duration_ms: u64,
    },
    LeafSelection {
        leaves: Vec<NodeId>,
    },
    SynthesisStart {
        leaf_count: usize,
        total_chars: usize,
    },
    SynthesisDone {
        input_tokens: u32,
        output_tokens: u32,
        duration_ms: u64,
    },
    BudgetExhausted {
        reason: String,
    },
}

/// Complete record of how a query was answered, for debugging and explainability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryTrace {
    pub query_id: String,
    pub question: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: u64,
    pub steps: Vec<TraceStep>,
    pub total_llm_calls: u32,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub bm25_candidates: Vec<SearchHit>,
    pub selected_leaves: Vec<NodeId>,
    pub final_citations: Vec<NodeId>,
}

/// Final answer returned by `ask` and `ask_in_doc`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
    pub trace: QueryTrace,
    /// Optional Verifiable Answer Receipt (Phase 39). Populated when an
    /// audit + receipt subsystem is configured; serialized as canonical
    /// JSON so downstream consumers can verify without depending on
    /// pagebridge-receipt directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_json: Option<serde_json::Value>,
}

/// Lower-level navigation result that omits the synthesis step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Navigation {
    pub selected_leaves: Vec<crate::record::NodeRecord>,
    pub trace: QueryTrace,
}

/// Counters returned by [`crate::adapter::StorageAdapter::stats`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AdapterStats {
    pub node_count: u64,
    pub document_count: u64,
    pub raw_bytes: u64,
    pub summary_cache_entries: u64,
}

/// Counters returned by the public facade.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PagebridgeStats {
    pub adapter: AdapterStats,
    pub adapter_name: String,
    pub llm_name: String,
    pub llm_model: String,
}

/// Configuration for navigation. Defaults are sensible for most cases.
#[derive(Debug, Clone, Copy)]
pub struct NavigationConfig {
    pub max_depth: u8,
    pub beam_width: u8,
    pub bm25_candidate_limit: usize,
    pub max_leaves: u8,
    pub max_llm_calls: u8,
    pub token_budget_per_query: u32,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        Self {
            max_depth: 4,
            beam_width: 3,
            bm25_candidate_limit: 30,
            max_leaves: 8,
            max_llm_calls: 12,
            token_budget_per_query: 32_000,
        }
    }
}

/// Optional trace persistence mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStorageMode {
    /// Do not persist traces. The trace is still returned in-band on every ask.
    None,
    /// Persist every trace through the adapter (Phase 14).
    Adapter,
}
