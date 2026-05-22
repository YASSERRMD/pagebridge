//! Python bindings for pagebridge.
//!
//! Exposes a small async-friendly subset of the Rust API. Build with maturin.

#![allow(
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::useless_conversion,
    clippy::unused_self,
    clippy::doc_markdown,
    clippy::doc_overindented_list_items,
    clippy::manual_let_else,
    clippy::needless_borrow,
    clippy::redundant_closure_for_method_calls,
    clippy::single_match_else,
    clippy::default_trait_access,
    clippy::use_self,
    clippy::elidable_lifetime_names,
    clippy::needless_lifetimes
)]

use std::sync::Arc;

use pagebridge::{DocId, IngestParams, Pagebridge, SourceKind, StorageAdapter};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio as pyo3_tokio;

fn map_err<E: std::fmt::Display>(e: E) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

#[pyclass(name = "Pagebridge", unsendable)]
pub struct PyPagebridge {
    inner: Pagebridge,
}

#[pymethods]
impl PyPagebridge {
    /// Open a SQLite-backed Pagebridge with the given Ollama URL and model.
    #[staticmethod]
    #[pyo3(signature = (path, ollama_url = "http://localhost:11434".to_owned(), model = "qwen2.5:7b".to_owned()))]
    pub fn open_sqlite<'py>(
        py: Python<'py>,
        path: String,
        ollama_url: String,
        model: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_tokio::future_into_py(py, async move {
            let storage: Arc<dyn StorageAdapter> = Arc::new(
                pagebridge::SqliteAdapter::open(&path)
                    .await
                    .map_err(map_err)?,
            );
            let llm = Arc::new(pagebridge::OllamaProvider::new(ollama_url, model));
            let bridge = Pagebridge::new(storage, llm).await.map_err(map_err)?;
            Ok(PyPagebridge { inner: bridge })
        })
    }

    /// Open an embedded Pagebridge with the given Ollama URL and model.
    #[staticmethod]
    #[pyo3(signature = (path, ollama_url = "http://localhost:11434".to_owned(), model = "qwen2.5:7b".to_owned()))]
    pub fn open_embedded<'py>(
        py: Python<'py>,
        path: String,
        ollama_url: String,
        model: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_tokio::future_into_py(py, async move {
            let storage: Arc<dyn StorageAdapter> =
                Arc::new(pagebridge::EmbeddedAdapter::open(&path).map_err(map_err)?);
            let llm = Arc::new(pagebridge::OllamaProvider::new(ollama_url, model));
            let bridge = Pagebridge::new(storage, llm).await.map_err(map_err)?;
            Ok(PyPagebridge { inner: bridge })
        })
    }

    /// Ingest a document. `kind` is one of `"markdown"`, `"plain"`, or `"pdf"`.
    #[pyo3(signature = (text, title, kind = "markdown".to_owned()))]
    pub fn ingest_document<'py>(
        &self,
        py: Python<'py>,
        text: Vec<u8>,
        title: String,
        kind: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bridge = self.inner.clone();
        pyo3_tokio::future_into_py(py, async move {
            let source = match kind.as_str() {
                "markdown" => SourceKind::Markdown,
                "plain" => SourceKind::Plain,
                "pdf" => SourceKind::Pdf,
                other => {
                    return Err(PyRuntimeError::new_err(format!(
                        "unknown source kind: {other}"
                    )))
                }
            };
            let handle = bridge
                .ingest_document(IngestParams {
                    title,
                    source_kind: source,
                    raw_text: text,
                    doc_id: None,
                    user_metadata: std::collections::BTreeMap::default(),
                })
                .await
                .map_err(map_err)?;
            let json = serde_json::to_string(&handle).map_err(map_err)?;
            Ok(json)
        })
    }

    /// Wait for background summary work to finish for the given doc id.
    pub fn wait_for_summaries<'py>(
        &self,
        py: Python<'py>,
        doc_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bridge = self.inner.clone();
        pyo3_tokio::future_into_py(py, async move {
            let id = DocId::new(doc_id).map_err(map_err)?;
            bridge.wait_for_summaries(&id).await.map_err(map_err)?;
            Ok(())
        })
    }

    /// Ask a question. Returns a JSON-serialized Answer.
    pub fn ask<'py>(&self, py: Python<'py>, question: String) -> PyResult<Bound<'py, PyAny>> {
        let bridge = self.inner.clone();
        pyo3_tokio::future_into_py(py, async move {
            let answer = bridge.ask(&question).await.map_err(map_err)?;
            serde_json::to_string(&answer).map_err(map_err)
        })
    }

    /// List ingested documents as a JSON array.
    pub fn list_documents<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let bridge = self.inner.clone();
        pyo3_tokio::future_into_py(py, async move {
            let docs = bridge.list_documents().await.map_err(map_err)?;
            serde_json::to_string(&docs).map_err(map_err)
        })
    }

    /// Remove a document.
    pub fn remove_document<'py>(
        &self,
        py: Python<'py>,
        doc_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bridge = self.inner.clone();
        pyo3_tokio::future_into_py(py, async move {
            let id = DocId::new(doc_id).map_err(map_err)?;
            bridge.remove_document(&id).await.map_err(map_err)?;
            Ok(())
        })
    }
}

#[pymodule]
fn _pagebridge(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", pagebridge_core::version())?;
    m.add_class::<PyPagebridge>()?;
    Ok(())
}
