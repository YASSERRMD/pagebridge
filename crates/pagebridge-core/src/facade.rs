//! The public `Pagebridge` facade.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation
)]

use std::sync::Arc;

use dashmap::DashMap;
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
    Answer, DocumentEntry, DocumentHandle, IngestParams, Navigation, NavigationConfig,
    PagebridgeStats, SearchHit, TraceStorageMode,
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
