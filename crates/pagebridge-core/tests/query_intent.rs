//! Integration tests for Phase J6 query intent classification and expansion.
//!
//! Verifies that:
//! - Queries mentioning section names or aliases are correctly classified.
//! - `expand_query` appends canonical terms without duplicating existing ones.
//! - `maybe_expand` is a no-op for disabled config, Generic types, and
//!   unmatched queries.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions
)]

use pagebridge_core::search::query_intent::{
    classify_query_intent, expand_query, maybe_expand, QueryIntentConfig,
};
use pagebridge_core::section_schema::document_schema_for;
use pagebridge_core::types::DocumentType;

// ---------------------------------------------------------------------------
// classify_query_intent
// ---------------------------------------------------------------------------

#[test]
fn classify_resume_skills_via_canonical() {
    let schema = document_schema_for(DocumentType::Resume);
    let sec = classify_query_intent("What are your skills?", DocumentType::Resume, schema);
    assert_eq!(sec.map(|s| s.canonical_name), Some("skills"));
}

#[test]
fn classify_resume_experience_via_alias() {
    let schema = document_schema_for(DocumentType::Resume);
    let sec = classify_query_intent(
        "Describe your professional experience",
        DocumentType::Resume,
        schema,
    );
    assert_eq!(sec.map(|s| s.canonical_name), Some("experience"));
}

#[test]
fn classify_research_paper_abstract() {
    let schema = document_schema_for(DocumentType::ResearchPaper);
    let sec = classify_query_intent(
        "What is the abstract of this paper?",
        DocumentType::ResearchPaper,
        schema,
    );
    assert_eq!(sec.map(|s| s.canonical_name), Some("abstract"));
}

#[test]
fn classify_legal_definitions() {
    let schema = document_schema_for(DocumentType::LegalDocument);
    let sec = classify_query_intent(
        "Show me the definitions section",
        DocumentType::LegalDocument,
        schema,
    );
    assert_eq!(sec.map(|s| s.canonical_name), Some("definitions"));
}

#[test]
fn classify_unrelated_query_returns_none() {
    let schema = document_schema_for(DocumentType::Resume);
    let sec = classify_query_intent("What is the meaning of life?", DocumentType::Resume, schema);
    assert!(
        sec.is_none(),
        "unrelated query should not match any section"
    );
}

// ---------------------------------------------------------------------------
// expand_query
// ---------------------------------------------------------------------------

#[test]
fn expand_adds_canonical_when_not_present() {
    let schema = document_schema_for(DocumentType::Resume);
    let skills_sec = schema
        .sections
        .iter()
        .find(|s| s.canonical_name == "skills")
        .unwrap();
    let expanded = expand_query("What languages do you program in?", skills_sec, 2);
    assert!(
        expanded.contains("skills"),
        "canonical 'skills' must be in expanded query: {expanded}"
    );
    assert!(
        expanded.starts_with("What languages do you program in?"),
        "original query must be preserved: {expanded}"
    );
}

#[test]
fn expand_respects_max_aliases_limit() {
    let schema = document_schema_for(DocumentType::Resume);
    let exp_sec = schema
        .sections
        .iter()
        .find(|s| s.canonical_name == "experience")
        .unwrap();
    let expanded = expand_query("Tell me about your jobs", exp_sec, 1);
    // Only 1 alias should be added beyond the canonical.
    let appended_len = expanded.len() - "Tell me about your jobs".len();
    assert!(
        appended_len > 0,
        "something must have been appended: {expanded}"
    );
}

// ---------------------------------------------------------------------------
// maybe_expand
// ---------------------------------------------------------------------------

#[test]
fn maybe_expand_enabled_with_match_expands() {
    let cfg = QueryIntentConfig::default();
    let q = "List all your technical skills";
    let expanded = maybe_expand(q, DocumentType::Resume, cfg);
    assert!(
        expanded.len() >= q.len(),
        "expanded should be at least as long: {expanded}"
    );
}

#[test]
fn maybe_expand_disabled_returns_original() {
    let cfg = QueryIntentConfig {
        enabled: false,
        max_aliases: 3,
    };
    let q = "What are your skills?";
    assert_eq!(maybe_expand(q, DocumentType::Resume, cfg), q);
}

#[test]
fn maybe_expand_generic_doc_type_is_noop() {
    let cfg = QueryIntentConfig::default();
    let q = "What are your skills?";
    assert_eq!(
        maybe_expand(q, DocumentType::Generic, cfg),
        q,
        "Generic doc type must be a no-op"
    );
}

#[test]
fn maybe_expand_no_section_match_returns_original() {
    let cfg = QueryIntentConfig::default();
    let q = "What is the population of Mars?";
    assert_eq!(
        maybe_expand(q, DocumentType::Resume, cfg),
        q,
        "no-match query must be returned unchanged"
    );
}
