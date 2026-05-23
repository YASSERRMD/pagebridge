//! pagebridge: cognitive retrieval for the database you already have.
//!
//! This umbrella crate re-exports the core types and wires up feature-gated
//! convenience constructors that pair a storage adapter with an LLM provider.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub use pagebridge_core::version;
pub use pagebridge_core::{
    AdapterStats, Answer, AnswerChunk, ChatMessage, ChatRole, Citation, CompletionRequest,
    CompletionResponse, CompletionStream, DocId, DocumentEntry, DocumentHandle, FinishReason,
    IngestParams, LlmConfig, LlmProvider, Navigation, NavigationConfig, NodeId, NodeLevel,
    NodeRecord, NodeSummary, Pagebridge, PagebridgeError, PagebridgeOptions, PagebridgeStats,
    PromptContext, PromptLibrary, QueryTrace, Result, SearchHit, SourceKind, StorageAdapter,
    StreamChunk, SummaryCacheEntry, TraceStep, TraceStorageMode,
};

#[cfg(feature = "embedded")]
pub use pagebridge_adapter_embedded::EmbeddedAdapter;
#[cfg(feature = "jsonfile")]
pub use pagebridge_adapter_jsonfile::JsonFileAdapter;
#[cfg(feature = "mongodb")]
pub use pagebridge_adapter_mongodb::MongoAdapter;
#[cfg(feature = "mssql")]
pub use pagebridge_adapter_mssql::MSSqlAdapter;
#[cfg(feature = "mysql")]
pub use pagebridge_adapter_mysql::MySqlAdapter;
#[cfg(feature = "oracle")]
pub use pagebridge_adapter_oracle::OracleAdapter;
#[cfg(feature = "postgres")]
pub use pagebridge_adapter_postgres::PostgresAdapter;
#[cfg(feature = "sqlite")]
pub use pagebridge_adapter_sqlite::SqliteAdapter;

#[cfg(feature = "admin")]
pub use pagebridge_admin as admin;
#[cfg(feature = "mcp")]
pub use pagebridge_mcp as mcp;
#[cfg(feature = "obs")]
pub use pagebridge_obs as obs;
#[cfg(feature = "anthropic")]
pub use pagebridge_llm_anthropic::AnthropicProvider;
#[cfg(feature = "llamacpp")]
pub use pagebridge_llm_llamacpp::{LlamaCppConfig, LlamaCppProvider};
#[cfg(feature = "ollama")]
pub use pagebridge_llm_ollama::OllamaProvider;
#[cfg(feature = "openai")]
pub use pagebridge_llm_openai::OpenAiCompatibleProvider;

use std::sync::Arc;

#[cfg(all(feature = "sqlite", feature = "ollama"))]
/// Quickstart: SQLite + Ollama.
pub async fn sqlite_with_ollama(
    path: impl AsRef<std::path::Path>,
    model: &str,
) -> Result<Pagebridge> {
    let storage = Arc::new(SqliteAdapter::open(path).await?);
    let llm = Arc::new(OllamaProvider::new(
        pagebridge_llm_ollama::DEFAULT_BASE_URL,
        model,
    ));
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "embedded", feature = "ollama"))]
/// Quickstart: embedded (redb + tantivy) + Ollama.
pub async fn embedded_with_ollama(
    path: impl AsRef<std::path::Path>,
    model: &str,
) -> Result<Pagebridge> {
    let storage = Arc::new(EmbeddedAdapter::open(path)?);
    let llm = Arc::new(OllamaProvider::new(
        pagebridge_llm_ollama::DEFAULT_BASE_URL,
        model,
    ));
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "postgres", feature = "openai"))]
/// Quickstart: Postgres + OpenAI (or any OpenAI-compatible endpoint).
pub async fn postgres_with_openai(url: &str, api_key: &str, model: &str) -> Result<Pagebridge> {
    let storage = Arc::new(PostgresAdapter::connect(url).await?);
    let llm = Arc::new(OpenAiCompatibleProvider::openai(api_key, model));
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "mongodb", feature = "anthropic"))]
/// Quickstart: MongoDB + Anthropic.
pub async fn mongo_with_anthropic(
    url: &str,
    db: &str,
    api_key: &str,
    model: &str,
) -> Result<Pagebridge> {
    let storage = Arc::new(MongoAdapter::connect(url, db).await?);
    let llm = Arc::new(AnthropicProvider::new(api_key, model));
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "sqlite", feature = "llamacpp"))]
/// Quickstart: SQLite + an embedded llama.cpp GGUF.
pub async fn sqlite_with_llamacpp(
    path: impl AsRef<std::path::Path>,
    gguf: impl AsRef<std::path::Path>,
) -> Result<Pagebridge> {
    let storage = Arc::new(SqliteAdapter::open(path).await?);
    let llm = Arc::new(LlamaCppProvider::from_gguf(gguf)?);
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "embedded", feature = "llamacpp"))]
/// Quickstart: embedded (redb + tantivy) + an embedded llama.cpp GGUF.
pub async fn embedded_with_llamacpp(
    path: impl AsRef<std::path::Path>,
    gguf: impl AsRef<std::path::Path>,
) -> Result<Pagebridge> {
    let storage = Arc::new(EmbeddedAdapter::open(path)?);
    let llm = Arc::new(LlamaCppProvider::from_gguf(gguf)?);
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "mysql", feature = "openai"))]
/// Quickstart: MySQL/MariaDB + OpenAI-compatible endpoint.
pub async fn mysql_with_openai(url: &str, api_key: &str, model: &str) -> Result<Pagebridge> {
    let storage = Arc::new(MySqlAdapter::connect(url).await?);
    let llm = Arc::new(OpenAiCompatibleProvider::openai(api_key, model));
    Pagebridge::new(storage, llm).await
}

#[cfg(all(feature = "jsonfile", feature = "ollama"))]
/// Quickstart: JSON-file prototyping store + Ollama.
pub async fn jsonfile_with_ollama(
    path: impl AsRef<std::path::Path>,
    model: &str,
) -> Result<Pagebridge> {
    let storage = Arc::new(JsonFileAdapter::open(path)?);
    let llm = Arc::new(OllamaProvider::new(
        pagebridge_llm_ollama::DEFAULT_BASE_URL,
        model,
    ));
    Pagebridge::new(storage, llm).await
}
