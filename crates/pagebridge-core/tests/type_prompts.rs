//! Tests for Phase J5 type-aware summary prompt selection.
//!
//! Verifies that `render_summarize` picks the most specific available
//! template for each (DocumentType, canonical_section) combination and
//! falls back gracefully when no specific template exists.

#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions
)]

use pagebridge_core::prompts::{PromptContext, PromptLibrary};
use pagebridge_core::types::DocumentType;

fn lib() -> PromptLibrary {
    PromptLibrary::v1()
}

fn ctx(doc_type: Option<DocumentType>, canonical_section: Option<&str>) -> PromptContext {
    PromptContext {
        document_title: Some("Test Node".into()),
        raw_text: Some("Some content here.".into()),
        document_type: doc_type,
        canonical_section: canonical_section.map(str::to_owned),
        ..PromptContext::default()
    }
}

// ---------------------------------------------------------------------------
// summarize_prompt_name tests (pure lookup, no rendering)
// ---------------------------------------------------------------------------

#[test]
fn prompt_name_resume_experience() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::Resume), Some("experience")),
        "summarize_resume_experience"
    );
}

#[test]
fn prompt_name_resume_skills() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::Resume), Some("skills")),
        "summarize_resume_skills"
    );
}

#[test]
fn prompt_name_resume_education() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::Resume), Some("education")),
        "summarize_resume_education"
    );
}

#[test]
fn prompt_name_research_paper_abstract() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::ResearchPaper), Some("abstract")),
        "summarize_research_paper_abstract"
    );
}

#[test]
fn prompt_name_research_paper_methodology() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::ResearchPaper), Some("methodology")),
        "summarize_research_paper_methodology"
    );
}

#[test]
fn prompt_name_legal_definitions() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::LegalDocument), Some("definitions")),
        "summarize_legal_document_definitions"
    );
}

#[test]
fn prompt_name_business_report_exec_summary() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(
            Some(DocumentType::BusinessReport),
            Some("executive_summary")
        ),
        "summarize_business_report_executive_summary"
    );
}

#[test]
fn prompt_name_meeting_action_items() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::MeetingMinutes), Some("action_items")),
        "summarize_meeting_minutes_action_items"
    );
}

#[test]
fn prompt_name_falls_back_to_generic_for_unknown_section() {
    let lib = lib();
    // "publications" has no specific resume prompt -> generic fallback
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::Resume), Some("publications")),
        "summarize"
    );
}

#[test]
fn prompt_name_falls_back_when_doc_type_none() {
    let lib = lib();
    assert_eq!(
        lib.summarize_prompt_name(None, Some("experience")),
        "summarize"
    );
}

#[test]
fn prompt_name_falls_back_when_section_none() {
    let lib = lib();
    // No type-level "summarize_resume" template; falls back to "summarize".
    assert_eq!(
        lib.summarize_prompt_name(Some(DocumentType::Resume), None),
        "summarize"
    );
}

// ---------------------------------------------------------------------------
// render_summarize tests (verifies rendered output contains key phrases)
// ---------------------------------------------------------------------------

#[test]
fn render_summarize_resume_experience_contains_job_titles_hint() {
    let lib = lib();
    let c = ctx(Some(DocumentType::Resume), Some("experience"));
    let rendered = lib.render_summarize(&c).unwrap();
    assert!(
        rendered.contains("job title")
            || rendered.contains("company")
            || rendered.contains("employment"),
        "resume/experience prompt must mention relevant fields"
    );
}

#[test]
fn render_summarize_research_abstract_contains_research_hint() {
    let lib = lib();
    let c = ctx(Some(DocumentType::ResearchPaper), Some("abstract"));
    let rendered = lib.render_summarize(&c).unwrap();
    assert!(
        rendered.contains("research")
            || rendered.contains("finding")
            || rendered.contains("methodology"),
        "research/abstract prompt must mention relevant fields"
    );
}

#[test]
fn render_summarize_generic_fallback_succeeds() {
    let lib = lib();
    let c = ctx(None, None);
    let rendered = lib.render_summarize(&c).unwrap();
    // Generic prompt exists and renders without error.
    assert!(!rendered.is_empty());
}

#[test]
fn render_summarize_unknown_section_falls_back_to_generic() {
    let lib = lib();
    let c = ctx(Some(DocumentType::Email), Some("nonexistent_section"));
    let rendered = lib.render_summarize(&c).unwrap();
    // Generic prompt is rendered.
    assert!(!rendered.is_empty());
}
