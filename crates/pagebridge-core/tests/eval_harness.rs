//! Phase J8: document-aware ingest evaluation harness.
//!
//! Ingests synthetic plain-text documents through the full J1-J7 pipeline
//! and verifies that section-targeting questions retrieve leaves from the
//! correct canonical section -- ideally via the J7 section-first fast-path
//! (zero BM25 / LLM navigation calls) when sections are cleanly tagged.
//!
//! The test doubles defined here are intentionally minimal: a scripted LLM
//! that returns a preset classifier verdict and a benign summarize shape.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    clippy::cast_lossless
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::{MemoryAdapter, StorageAdapter};
use pagebridge_core::error::Result;
use pagebridge_core::ingest::{ingest_full, ClassifyConfig};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::search::{navigate, NavigationOutcome};
use pagebridge_core::trace::TraceBuilder;
use pagebridge_core::types::{DocumentType, IngestParams, NavigationConfig, SourceKind};
use pagebridge_core::SummaryWorkerConfig;

// ---------------------------------------------------------------------------
// Synthetic corpora
// ---------------------------------------------------------------------------

/// Plain-text resume with sections recognisable by the J2 Resume schema.
const SYNTHETIC_RESUME: &str = "\
skills\n\
Rust, Python, Kubernetes, AWS, PostgreSQL.\n\
\n\
experience\n\
Acme Corp 2020-2024 -- Software Engineer.\n\
Built distributed systems serving 10M requests per day.\n\
Led a team of 4 engineers; delivered 3 major product launches.\n\
\n\
education\n\
BSc Computer Science, State University, 2019.\n\
GPA 3.8; awarded Dean List in junior and senior years.\n\
";

/// Plain-text research paper with sections recognisable by the J2 ResearchPaper schema.
const SYNTHETIC_PAPER: &str = "\
abstract\n\
We present a new method for lossless text compression.\n\
Our approach reduces corpus size by 40 percent compared to gzip.\n\
\n\
methodology\n\
We apply a Huffman coding variant with dynamic dictionaries.\n\
The algorithm runs in O(n log n) time on average.\n\
\n\
results\n\
Benchmark on a 1 GB Wikipedia dump yields 42 percent compression.\n\
Our method outperforms gzip, zstd, and brotli on natural-language text.\n\
";

// ---------------------------------------------------------------------------
// Scripted LLM: injects a fixed classify verdict; benign for summaries.
// ---------------------------------------------------------------------------

struct ScriptedLlm {
    classify_verdict: serde_json::Value,
}

impl ScriptedLlm {
    fn for_doc_type(doc_type: &str) -> Self {
        Self {
            classify_verdict: serde_json::json!({
                "document_type": doc_type,
                "confidence": 0.95,
                "reasons": ["eval harness scripted verdict"]
            }),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
    fn name(&self) -> &'static str {
        "scripted-eval"
    }
    fn model(&self) -> &'static str {
        "eval-1"
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
        })
    }
    async fn complete_json(
        &self,
        _req: CompletionRequest,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Detect the classify schema by checking for "document_type" property.
        let is_classify = schema
            .get("properties")
            .and_then(|p| p.get("document_type"))
            .is_some();
        if is_classify {
            return Ok(self.classify_verdict.clone());
        }
        // Summarize calls: return a benign shaped object so fan-out completes.
        Ok(serde_json::json!({
            "title": "section",
            "routing_summary": "routing",
            "summary": "summary text",
            "keywords": []
        }))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn ingest_doc(text: &str, title: &str, doc_type_str: &str) -> Arc<MemoryAdapter> {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(ScriptedLlm::for_doc_type(doc_type_str));
    let prompts = Arc::new(PromptLibrary::v1());
    let params = IngestParams {
        title: title.to_owned(),
        source_kind: SourceKind::Plain,
        raw_text: text.as_bytes().to_vec(),
        doc_id: None,
        user_metadata: BTreeMap::default(),
    };
    let (_handle, join) = ingest_full(
        storage.clone(),
        llm,
        prompts,
        params,
        SummaryWorkerConfig::default(),
        ClassifyConfig {
            enabled: true,
            min_confidence: 0.5,
            sample_chars: 4000,
            vision_peek: false,
        },
        None,
    )
    .await
    .unwrap();
    join.await.unwrap().unwrap();
    storage
}

async fn ask(
    storage: &Arc<MemoryAdapter>,
    question: &str,
    llm: &Arc<dyn LlmProvider>,
) -> NavigationOutcome {
    let storage_dyn: Arc<dyn StorageAdapter> = storage.clone();
    let docs = storage.list_documents().await.unwrap();
    let doc_id = docs.first().map(|d| d.doc_id.clone());
    let prompts = PromptLibrary::v1();
    let mut trace = TraceBuilder::new(question);
    navigate(
        &storage_dyn,
        llm,
        &prompts,
        question,
        NavigationConfig::default(),
        doc_id.as_ref(),
        &mut trace,
    )
    .await
    .unwrap()
}

/// A no-op LLM provider for the navigate step.
/// J7 section-first fast-path should never call it for matched queries.
struct NullLlm;

#[async_trait]
impl LlmProvider for NullLlm {
    fn name(&self) -> &'static str {
        "null"
    }
    fn model(&self) -> &'static str {
        "null-1"
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
        })
    }
    async fn complete_json(
        &self,
        _req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Should not be called when J7 fast-path fires.
        Ok(serde_json::json!({"action": "select_leaves", "node_ids": []}))
    }
}

// ---------------------------------------------------------------------------
// Resume evaluation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn eval_resume_doc_type_is_persisted() {
    let storage = ingest_doc(SYNTHETIC_RESUME, "Test Resume", "resume").await;
    let docs = storage.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(
        docs[0].document_type,
        Some(DocumentType::Resume),
        "document_type must be persisted after ingest"
    );
}

#[tokio::test]
async fn eval_resume_skills_section_tagged() {
    let storage = ingest_doc(SYNTHETIC_RESUME, "Test Resume", "resume").await;
    let docs = storage.list_documents().await.unwrap();
    let doc_id = &docs[0].doc_id;

    let skills_leaves = storage
        .find_nodes_by_canonical(doc_id, "skills")
        .await
        .unwrap();
    assert!(
        !skills_leaves.is_empty(),
        "skills section must be tagged and have leaves after ingest"
    );
}

#[tokio::test]
async fn eval_resume_experience_section_tagged() {
    let storage = ingest_doc(SYNTHETIC_RESUME, "Test Resume", "resume").await;
    let docs = storage.list_documents().await.unwrap();
    let doc_id = &docs[0].doc_id;

    let exp_leaves = storage
        .find_nodes_by_canonical(doc_id, "experience")
        .await
        .unwrap();
    assert!(
        !exp_leaves.is_empty(),
        "experience section must be tagged and have leaves after ingest"
    );
}

#[tokio::test]
async fn eval_resume_navigate_skills_query_returns_leaves() {
    let storage = ingest_doc(SYNTHETIC_RESUME, "Test Resume", "resume").await;
    let llm: Arc<dyn LlmProvider> = Arc::new(NullLlm);
    let outcome = ask(&storage, "List your technical skills", &llm).await;
    assert!(
        !outcome.selected_leaves.is_empty(),
        "skills query must return at least one leaf"
    );
    // J7 fast-path: bm25_candidates should be empty (section jump, no BM25).
    assert!(
        outcome.bm25_candidates.is_empty(),
        "J7 fast-path fired so bm25_candidates must be empty"
    );
}

#[tokio::test]
async fn eval_resume_navigate_experience_query_returns_leaves() {
    let storage = ingest_doc(SYNTHETIC_RESUME, "Test Resume", "resume").await;
    let llm: Arc<dyn LlmProvider> = Arc::new(NullLlm);
    let outcome = ask(&storage, "What is your work experience?", &llm).await;
    assert!(
        !outcome.selected_leaves.is_empty(),
        "experience query must return at least one leaf"
    );
    assert!(
        outcome.bm25_candidates.is_empty(),
        "J7 fast-path fired so bm25_candidates must be empty"
    );
}

// ---------------------------------------------------------------------------
// Research paper evaluation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn eval_paper_doc_type_is_persisted() {
    let storage = ingest_doc(SYNTHETIC_PAPER, "Test Paper", "research_paper").await;
    let docs = storage.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(
        docs[0].document_type,
        Some(DocumentType::ResearchPaper),
        "document_type must be persisted as ResearchPaper"
    );
}

#[tokio::test]
async fn eval_paper_abstract_section_tagged() {
    let storage = ingest_doc(SYNTHETIC_PAPER, "Test Paper", "research_paper").await;
    let docs = storage.list_documents().await.unwrap();
    let doc_id = &docs[0].doc_id;

    let abstract_leaves = storage
        .find_nodes_by_canonical(doc_id, "abstract")
        .await
        .unwrap();
    assert!(
        !abstract_leaves.is_empty(),
        "abstract section must be tagged and have leaves"
    );
}

#[tokio::test]
async fn eval_paper_navigate_abstract_query_returns_leaves() {
    let storage = ingest_doc(SYNTHETIC_PAPER, "Test Paper", "research_paper").await;
    let llm: Arc<dyn LlmProvider> = Arc::new(NullLlm);
    let outcome = ask(&storage, "What is the abstract of this paper?", &llm).await;
    assert!(
        !outcome.selected_leaves.is_empty(),
        "abstract query must return at least one leaf"
    );
    assert!(
        outcome.bm25_candidates.is_empty(),
        "J7 fast-path fired so bm25_candidates must be empty"
    );
}

#[tokio::test]
async fn eval_paper_navigate_methodology_query_returns_leaves() {
    let storage = ingest_doc(SYNTHETIC_PAPER, "Test Paper", "research_paper").await;
    let llm: Arc<dyn LlmProvider> = Arc::new(NullLlm);
    let outcome = ask(&storage, "Describe the methodology used", &llm).await;
    assert!(
        !outcome.selected_leaves.is_empty(),
        "methodology query must return at least one leaf"
    );
    assert!(
        outcome.bm25_candidates.is_empty(),
        "J7 fast-path fired so bm25_candidates must be empty"
    );
}
