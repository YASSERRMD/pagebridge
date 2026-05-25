//! Integration tests for Phase J3 canonical section tagging.
//!
//! Verifies that `tag_section_nodes` correctly stamps section-level
//! NodeRecords with their canonical section names and alias lists,
//! and that the tags round-trip through a full `ingest_full` pipeline.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::unnecessary_literal_bound,
    clippy::struct_field_names
)]

use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::error::Result;
use pagebridge_core::ingest::{ingest_full, tag_section_nodes, ClassifyConfig};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::{DocumentType, IngestParams, SourceKind};
use pagebridge_core::{DocId, NodeId, StorageAdapter, SummaryWorkerConfig};

// ---------------------------------------------------------------------------
// Unit tests for the tag_section_nodes helper (pure, no IO)
// ---------------------------------------------------------------------------

fn make_section_node(title: &str, level: NodeLevel) -> NodeRecord {
    let doc_id = DocId::new("test-doc-abc123".to_owned()).unwrap();
    let root_id = NodeId::root(&doc_id);
    let node_id = root_id.child("sec", "1").unwrap();
    NodeRecord {
        node_id,
        doc_id,
        parent_id: None,
        title: title.to_owned(),
        level,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: None,
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: false,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
        canonical_section: None,
        section_aliases: vec![],
    }
}

#[test]
fn tag_section_nodes_resume_experience() {
    let mut nodes = vec![make_section_node("Work Experience", NodeLevel::Section)];
    tag_section_nodes(&mut nodes, Some(DocumentType::Resume));
    assert_eq!(
        nodes[0].canonical_section.as_deref(),
        Some("experience"),
        "Work Experience must map to experience"
    );
    assert!(
        !nodes[0].section_aliases.is_empty(),
        "aliases must be populated"
    );
}

#[test]
fn tag_section_nodes_resume_skills_exact() {
    let mut nodes = vec![make_section_node("skills", NodeLevel::Section)];
    tag_section_nodes(&mut nodes, Some(DocumentType::Resume));
    assert_eq!(nodes[0].canonical_section.as_deref(), Some("skills"));
}

#[test]
fn tag_section_nodes_case_insensitive() {
    let mut nodes = vec![make_section_node("SKILLS", NodeLevel::Section)];
    tag_section_nodes(&mut nodes, Some(DocumentType::Resume));
    assert_eq!(nodes[0].canonical_section.as_deref(), Some("skills"));
}

#[test]
fn tag_section_nodes_no_match_leaves_none() {
    let mut nodes = vec![make_section_node("Hobbies", NodeLevel::Section)];
    tag_section_nodes(&mut nodes, Some(DocumentType::Resume));
    assert_eq!(nodes[0].canonical_section, None);
    assert!(nodes[0].section_aliases.is_empty());
}

#[test]
fn tag_section_nodes_disabled_when_doc_type_none() {
    let mut nodes = vec![make_section_node("Work Experience", NodeLevel::Section)];
    tag_section_nodes(&mut nodes, None);
    assert_eq!(nodes[0].canonical_section, None);
}

#[test]
fn tag_section_nodes_skips_leaf_nodes() {
    let doc_id = DocId::new("test-doc-abc123".to_owned()).unwrap();
    let root_id = NodeId::root(&doc_id);
    let node_id = root_id.child("leaf", "1").unwrap();
    let mut leaf = NodeRecord {
        node_id,
        doc_id,
        parent_id: None,
        title: "Work Experience".to_owned(),
        level: NodeLevel::Leaf,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: Some((0, 10)),
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
        canonical_section: None,
        section_aliases: vec![],
    };
    tag_section_nodes(std::slice::from_mut(&mut leaf), Some(DocumentType::Resume));
    assert_eq!(
        leaf.canonical_section, None,
        "leaf nodes must not be tagged"
    );
}

#[test]
fn tag_section_nodes_research_paper_methodology_alias() {
    let mut nodes = vec![make_section_node(
        "Materials and Methods",
        NodeLevel::Section,
    )];
    tag_section_nodes(&mut nodes, Some(DocumentType::ResearchPaper));
    assert_eq!(nodes[0].canonical_section.as_deref(), Some("methodology"));
}

#[test]
fn tag_section_nodes_subsection_is_tagged() {
    let mut nodes = vec![make_section_node("Technical Skills", NodeLevel::Subsection)];
    tag_section_nodes(&mut nodes, Some(DocumentType::Resume));
    assert_eq!(nodes[0].canonical_section.as_deref(), Some("skills"));
}

// ---------------------------------------------------------------------------
// Integration test: tags survive a full ingest_full pipeline
// ---------------------------------------------------------------------------

struct ScriptedLlm;

#[async_trait]
impl LlmProvider for ScriptedLlm {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn model(&self) -> &str {
        "scripted-1"
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
        let is_classify = schema
            .get("properties")
            .and_then(|p| p.get("document_type"))
            .is_some();
        if is_classify {
            return Ok(serde_json::json!({
                "document_type": "resume",
                "confidence": 0.95,
                "reasons": ["experience section", "skills section"]
            }));
        }
        Ok(serde_json::json!({
            "title": "ok",
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }))
    }
}

#[tokio::test]
async fn section_tags_persisted_after_ingest_full() {
    let llm = Arc::new(ScriptedLlm);
    let storage = Arc::new(MemoryAdapter::new());
    let prompts = Arc::new(PromptLibrary::v1());
    // A minimal resume-like markdown. Use H1 headings so the parser emits
    // NodeLevel::Section nodes as direct children of the root, making it
    // straightforward to assert on canonical_section without recursion.
    let raw =
        b"# Skills\nRust, Go, Python.\n\n# Work Experience\nAcme Corp 2020-2024.\n\n# Education\nBSc Computer Science.\n";
    let params = IngestParams {
        title: "Jane Smith Resume".into(),
        source_kind: SourceKind::Markdown,
        raw_text: raw.to_vec(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
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

    // Pull all nodes and find section-level ones via root's direct children.
    let doc = storage.list_documents().await.unwrap();
    assert_eq!(doc.len(), 1);
    let root_id = &doc[0].root_node_id;
    // Direct children of root are the section nodes in a markdown parse.
    let sections = storage.children_records(root_id).await.unwrap();
    assert!(!sections.is_empty(), "must have at least one section node");

    // At least one section whose heading matches a resume schema alias must be tagged.
    let has_tagged = sections.iter().any(|n| n.canonical_section.is_some());
    assert!(
        has_tagged,
        "at least one section must carry a canonical_section tag"
    );
}
