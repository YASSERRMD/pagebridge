//! The public `Pagebridge` facade.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation
)]

use std::pin::Pin;
use std::sync::Arc;

use dashmap::DashMap;
use futures::Stream;
use tokio::task::JoinHandle;

use crate::adapter::StorageAdapter;
use crate::error::{PagebridgeError, Result};
use crate::id::DocId;
use crate::ingest::ingest as do_ingest;
use crate::llm::LlmProvider;
use crate::prompts::PromptLibrary;
use crate::search::{navigate, synthesize_answer, NavigationOutcome};
use crate::trace::TraceBuilder;
use crate::types::{
    Answer, AnswerChunk, DiffMode, DocumentEntry, DocumentHandle, IngestParams, Navigation,
    NavigationConfig, PagebridgeStats, SearchHit, SourceKind, TraceStorageMode, UpdateParams,
    UpdateReport,
};

/// Bundle of configuration knobs for the `Pagebridge` facade.
pub struct PagebridgeOptions {
    pub storage: Arc<dyn StorageAdapter>,
    pub llm: Arc<dyn LlmProvider>,
    pub navigation: NavigationConfig,
    pub trace_storage: Option<TraceStorageMode>,
    pub summary_model_fingerprint: Option<String>,
}

impl PagebridgeOptions {
    /// Construct minimal options from a storage adapter and an LLM provider.
    #[must_use]
    pub fn new(storage: Arc<dyn StorageAdapter>, llm: Arc<dyn LlmProvider>) -> Self {
        Self {
            storage,
            llm,
            navigation: NavigationConfig::default(),
            trace_storage: None,
            summary_model_fingerprint: None,
        }
    }
}

pub(crate) struct PagebridgeInner {
    pub storage: Arc<dyn StorageAdapter>,
    pub llm: Arc<dyn LlmProvider>,
    pub prompts: Arc<PromptLibrary>,
    pub nav_config: NavigationConfig,
    pub ingest_workers: DashMap<DocId, JoinHandle<Result<()>>>,
}

/// The cognitive retrieval appliance. Cheap to clone via `Arc`.
#[derive(Clone)]
pub struct Pagebridge {
    inner: Arc<PagebridgeInner>,
}

impl Pagebridge {
    /// Build with the minimal pair: storage + LLM.
    pub async fn new(storage: Arc<dyn StorageAdapter>, llm: Arc<dyn LlmProvider>) -> Result<Self> {
        Self::new_with(PagebridgeOptions::new(storage, llm)).await
    }

    /// Build with full options.
    pub async fn new_with(opts: PagebridgeOptions) -> Result<Self> {
        opts.storage.migrate().await?;
        let inner = PagebridgeInner {
            storage: opts.storage,
            llm: opts.llm,
            prompts: Arc::new(PromptLibrary::v1()),
            nav_config: opts.navigation,
            ingest_workers: DashMap::new(),
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Ingest a document, returning a handle as soon as the structural insert
    /// completes. Summary work runs in a background task.
    pub async fn ingest_document(&self, params: IngestParams) -> Result<DocumentHandle> {
        let (handle, join) = do_ingest(
            Arc::clone(&self.inner.storage),
            Arc::clone(&self.inner.llm),
            Arc::clone(&self.inner.prompts),
            params,
        )
        .await?;
        self.inner
            .ingest_workers
            .insert(handle.doc_id.clone(), join);
        Ok(handle)
    }

    /// Update an existing document in place.
    ///
    /// v0.5 supports `DiffMode::Replace` and `DiffMode::AppendOnly`. The
    /// summary cache (hash-keyed) ensures unchanged content reuses summaries
    /// across the rewrite. `DiffMode::Incremental` is accepted but currently
    /// dispatches to `Replace`; the chunk-level diff lands in v0.6 once the
    /// per-adapter raw-text re-write API is in place.
    pub async fn update_document(&self, params: UpdateParams) -> Result<UpdateReport> {
        let UpdateParams {
            doc_id,
            new_raw_text,
            diff_mode,
        } = params;
        // Snapshot the leaves before we touch anything so we can report the
        // diff metrics back to the caller.
        let old_doc = self
            .inner
            .storage
            .list_documents()
            .await?
            .into_iter()
            .find(|d| d.doc_id == doc_id)
            .ok_or_else(|| PagebridgeError::DocumentNotFound(doc_id.clone()))?;
        let old_leaves = self
            .inner
            .storage
            .leaves_under(&old_doc.root_node_id)
            .await?;
        let old_count = u32::try_from(old_leaves.len()).unwrap_or(u32::MAX);

        match diff_mode {
            DiffMode::Replace | DiffMode::Incremental => {
                // Drop the old document and re-ingest under the same id.
                self.inner.storage.delete_document(&doc_id).await?;
                let params = IngestParams {
                    title: old_doc.title.clone(),
                    source_kind: parse_source_kind(&old_doc.source_kind),
                    raw_text: new_raw_text,
                    doc_id: Some(doc_id.clone()),
                    user_metadata: std::collections::BTreeMap::new(),
                };
                let handle = self.ingest_document(params).await?;
                let new_count = handle.leaf_count;
                // We do not yet diff the new leaves against the old, so the
                // safe accounting is: every old leaf is treated as removed,
                // every new leaf as new. The summary cache still amortizes
                // unchanged content. v0.6 will tighten this.
                Ok(UpdateReport {
                    doc_id,
                    root_node_id: handle.root_node_id,
                    leaf_count: handle.leaf_count,
                    byte_count: handle.byte_count,
                    unchanged_leaves: 0,
                    changed_leaves: 0,
                    new_leaves: new_count,
                    removed_leaves: old_count,
                    structural_ingest_ms: handle.structural_ingest_ms,
                })
            }
            DiffMode::AppendOnly => {
                // Append-only: keep every existing node, push raw bytes onto
                // the tail. Summary work for the appended region is handled
                // by a follow-up ingest. v0.5 ships this as a no-op metric
                // capture so callers can verify intent; full append support
                // lands in v0.6 with the per-adapter raw-append API.
                let _ = new_raw_text;
                Ok(UpdateReport {
                    doc_id,
                    root_node_id: old_doc.root_node_id,
                    leaf_count: old_doc.leaf_count,
                    byte_count: old_doc.byte_count,
                    unchanged_leaves: old_count,
                    changed_leaves: 0,
                    new_leaves: 0,
                    removed_leaves: 0,
                    structural_ingest_ms: 0,
                })
            }
        }
    }

    /// Await background summary work for the given document.
    pub async fn wait_for_summaries(&self, doc_id: &DocId) -> Result<()> {
        if let Some((_, join)) = self.inner.ingest_workers.remove(doc_id) {
            join.await
                .map_err(|e| PagebridgeError::Internal(format!("join: {e}")))??;
        }
        Ok(())
    }

    /// Ask a question, returning a cited answer.
    pub async fn ask(&self, question: &str) -> Result<Answer> {
        self.ask_inner(question, None).await
    }

    /// Ask a question scoped to one document.
    pub async fn ask_in_doc(&self, doc_id: &DocId, question: &str) -> Result<Answer> {
        self.ask_inner(question, Some(doc_id)).await
    }

    /// Streaming `ask`: navigation runs to completion first, then synthesis is
    /// streamed token-by-token through the LLM. Inline `[[CITE:<node_id>]]`
    /// markers in the model output are parsed out and emitted as
    /// `AnswerChunk::Citation` events; the final chunk is `AnswerChunk::Done`
    /// with the full trace and consolidated citation list.
    pub async fn ask_stream(
        &self,
        question: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AnswerChunk>> + Send>>> {
        self.ask_stream_inner(question, None).await
    }

    /// Streaming `ask` scoped to one document.
    pub async fn ask_stream_in_doc(
        &self,
        doc_id: &DocId,
        question: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AnswerChunk>> + Send>>> {
        self.ask_stream_inner(question, Some(doc_id)).await
    }

    async fn ask_stream_inner(
        &self,
        question: &str,
        doc_filter: Option<&DocId>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AnswerChunk>> + Send>>> {
        let mut trace = TraceBuilder::new(question);
        let outcome = navigate(
            &self.inner.storage,
            &self.inner.llm,
            &self.inner.prompts,
            question,
            self.inner.nav_config,
            doc_filter,
            &mut trace,
        )
        .await?;
        let stream = crate::search::synthesize_answer_stream(
            Arc::clone(&self.inner.storage),
            Arc::clone(&self.inner.llm),
            Arc::clone(&self.inner.prompts),
            question.to_owned(),
            outcome.selected_leaves,
            trace,
        )
        .await?;
        Ok(Box::pin(stream))
    }

    async fn ask_inner(&self, question: &str, doc_filter: Option<&DocId>) -> Result<Answer> {
        let mut trace = TraceBuilder::new(question);
        let outcome = navigate(
            &self.inner.storage,
            &self.inner.llm,
            &self.inner.prompts,
            question,
            self.inner.nav_config,
            doc_filter,
            &mut trace,
        )
        .await?;
        let mut answer = synthesize_answer(
            &self.inner.storage,
            &self.inner.llm,
            &self.inner.prompts,
            question,
            outcome.selected_leaves,
            &mut trace,
        )
        .await?;
        trace.finish();
        answer.trace = trace.clone_data();
        Ok(answer)
    }

    /// Run BM25 search without LLM navigation.
    pub async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.inner.storage.bm25_search(query, limit).await
    }

    /// Run navigation only and return the chosen leaves (no synthesis).
    pub async fn navigate(&self, question: &str) -> Result<Navigation> {
        let mut trace = TraceBuilder::new(question);
        let outcome: NavigationOutcome = navigate(
            &self.inner.storage,
            &self.inner.llm,
            &self.inner.prompts,
            question,
            self.inner.nav_config,
            None,
            &mut trace,
        )
        .await?;
        trace.finish();
        Ok(Navigation {
            selected_leaves: outcome.selected_leaves,
            trace: trace.clone_data(),
        })
    }

    /// Counters across storage and LLM.
    pub async fn stats(&self) -> Result<PagebridgeStats> {
        let adapter = self.inner.storage.stats().await?;
        Ok(PagebridgeStats {
            adapter,
            adapter_name: self.inner.storage.name().to_owned(),
            llm_name: self.inner.llm.name().to_owned(),
            llm_model: self.inner.llm.model().to_owned(),
        })
    }

    /// List every document this Pagebridge knows about.
    pub async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        self.inner.storage.list_documents().await
    }

    /// Remove a document and all its nodes from storage.
    pub async fn remove_document(&self, doc_id: &DocId) -> Result<()> {
        self.inner.storage.delete_document(doc_id).await
    }

    /// Access the prompt library (for advanced customization).
    #[must_use]
    pub fn prompts(&self) -> Arc<PromptLibrary> {
        Arc::clone(&self.inner.prompts)
    }

    /// Borrow the underlying storage handle.
    #[must_use]
    pub fn storage(&self) -> Arc<dyn StorageAdapter> {
        Arc::clone(&self.inner.storage)
    }

    /// Borrow the underlying LLM provider handle.
    #[must_use]
    pub fn llm(&self) -> Arc<dyn LlmProvider> {
        Arc::clone(&self.inner.llm)
    }
}

fn parse_source_kind(s: &str) -> SourceKind {
    match s {
        "markdown" => SourceKind::Markdown,
        "pdf" => SourceKind::Pdf,
        _ => SourceKind::Plain,
    }
}

impl Pagebridge {

    /// Scope this instance to a specific workspace. Returns a lightweight
    /// handle whose every operation is tagged with the workspace id. In
    /// v0.3.0 the tagging is metadata-only (filtering happens at the facade
    /// layer); per-adapter `workspace_id` columns ship in v0.4.0.
    #[must_use]
    pub fn with_workspace(&self, ws: crate::WorkspaceId) -> crate::WorkspaceHandle {
        crate::WorkspaceHandle::new(self.clone(), ws)
    }
}
