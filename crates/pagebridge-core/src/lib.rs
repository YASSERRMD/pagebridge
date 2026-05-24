//! pagebridge-core: cognitive retrieval primitives.
//!
//! This crate hosts the core types, the `StorageAdapter` trait, the `LlmProvider`
//! trait, the prompt library, the ingest pipeline, and the navigation/synthesis
//! engine. Higher-level crates (adapters, LLM providers, the umbrella crate)
//! depend on this one.

pub mod adapter;
pub mod audit_hook;
pub mod citation;
pub mod error;
pub mod facade;
pub mod id;
pub mod ingest;
pub mod llm;
pub mod llm_policy;
pub mod prompts;
pub mod record;
pub mod search;
pub mod replication;
pub mod trace;
pub mod types;
pub mod workspace;
pub mod workspace_handle;

pub use audit_hook::{
    noop as noop_audit_hook, noop_receipt_issuer, AskAuditFields, AuditHook, DeleteAuditFields,
    IngestAuditFields, NoopAuditHook, NoopReceiptIssuer, ReceiptIssuanceInputs, ReceiptIssuer,
};
pub use facade::{Pagebridge, PagebridgeOptions};
pub use ingest::{
    BatchWriterConfig, IngestStage, ProgressSnapshot, ProgressTracker, SummaryTask,
    SummaryWorkerConfig, WriterStats,
};

pub use adapter::StorageAdapter;
pub use error::{PagebridgeError, Result};
pub use id::{DocId, NodeId};
pub use llm::{
    ChatMessage, ChatRole, CompletionRequest, CompletionResponse, CompletionStream, FinishReason,
    LlmConfig, LlmProvider, RateLimits, StreamChunk, VisionImage,
};
pub use llm_policy::{
    parse_retry_after, CircuitBreaker, CircuitBreakerConfig, LlmCallPolicy, RetryClass,
};
pub use prompts::{PromptContext, PromptLibrary};
pub use replication::{
    InvalidationEvent, InvalidationKind, ReplicationConfig, ReplicationRole,
};
pub use workspace::WorkspaceId;
pub use workspace_handle::WorkspaceHandle;
pub use record::{NodeLevel, NodeRecord, NodeSummary};
pub use types::{
    AdapterStats, Answer, AnswerChunk, Citation, DiffMode, DocumentEntry, DocumentHandle,
    DocumentIngestHandle, IngestParams, Navigation, NavigationConfig, PagebridgeStats, QueryTrace,
    SearchHit, SourceKind, SummaryCacheEntry, TraceStep, TraceStorageMode, UpdateParams,
    UpdateReport,
};

/// Crate version string, used by the CLI and by trace metadata.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
