//! Tests for the prompt templater and the v1 prompt bundle.

#![allow(clippy::redundant_clone)]

use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::prompts::{PromptContext, PromptLibrary};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};

fn ctx_question() -> PromptContext {
    let doc = DocId::new("policy").unwrap();
    let root = NodeId::root(&doc);
    let sec1 = root.child("sec", "1").unwrap();
    let sec2 = root.child("sec", "2").unwrap();
    PromptContext {
        question: Some("What is the implementation timeline?".into()),
        children: vec![
            NodeSummary {
                node_id: sec1.clone(),
                parent_id: Some(root.clone()),
                title: "Section 1: Overview".into(),
                level: NodeLevel::Section,
                routing_summary: "introductory overview of the policy goals".into(),
                is_leaf: false,
            },
            NodeSummary {
                node_id: sec2,
                parent_id: Some(root),
                title: "Section 2: Implementation".into(),
                level: NodeLevel::Section,
                routing_summary: "phased rollout schedule with milestones".into(),
                is_leaf: false,
            },
        ],
        document_title: Some("Carbon Policy 2026".into()),
        current_path: vec![],
        ..Default::default()
    }
}

#[test]
fn navigate_renders_without_remnants() {
    let lib = PromptLibrary::v1();
    let s = lib.render("navigate", &ctx_question()).unwrap();
    assert!(s.contains("What is the implementation timeline?"));
    assert!(s.contains("Section 2: Implementation"));
    assert!(s.contains("phased rollout schedule with milestones"));
    assert!(!s.contains("{{"));
    assert!(!s.contains("{%"));
}

#[test]
fn navigate_handles_empty_children() {
    let lib = PromptLibrary::v1();
    let mut ctx = ctx_question();
    ctx.children.clear();
    let s = lib.render("navigate", &ctx).unwrap();
    assert!(s.contains("What is the implementation timeline?"));
    assert!(!s.contains("{%"));
}

#[test]
fn summarize_renders() {
    let lib = PromptLibrary::v1();
    let ctx = PromptContext {
        document_title: Some("Section 1.2".into()),
        raw_text: Some("Long body of section 1.2 ...".into()),
        ..Default::default()
    };
    let s = lib.render("summarize", &ctx).unwrap();
    assert!(s.contains("Section 1.2"));
    assert!(s.contains("Long body of section 1.2"));
    assert!(!s.contains("{{"));
}

#[test]
fn synthesize_renders_with_leaves() {
    let lib = PromptLibrary::v1();
    let doc = DocId::new("policy").unwrap();
    let root = NodeId::root(&doc);
    let leaf = root.child("leaf", "1").unwrap();
    let ctx = PromptContext {
        question: Some("How long does rollout take?".into()),
        leaves: vec![NodeRecord {
            node_id: leaf,
            doc_id: doc,
            parent_id: Some(root),
            title: "Rollout timeline".into(),
            level: NodeLevel::Leaf,
            routing_summary: String::new(),
            summary: "Phase 1 launches in Q1 2026 and completes by Q4 2027.".into(),
            child_ids: vec![],
            span: Some((0, 60)),
            page_start: None,
            page_end: None,
            keywords: vec!["timeline".into()],
            is_leaf: true,
            created_at: 0,
            updated_at: 0,
            source_hash: [0; 32],
            canonical_section: None,
            section_aliases: vec![],
        }],
        ..Default::default()
    };
    let s = lib.render("synthesize", &ctx).unwrap();
    assert!(s.contains("How long does rollout take?"));
    assert!(s.contains("Rollout timeline"));
    assert!(s.contains("Phase 1 launches"));
    assert!(s.contains("<citations>"));
}

#[test]
fn keywords_template_unicode_safe() {
    let lib = PromptLibrary::v1();
    let ctx = PromptContext {
        raw_text: Some("شركة الإمارات للطاقة and renewable mandates".into()),
        ..Default::default()
    };
    let s = lib.render("keywords", &ctx).unwrap();
    assert!(s.contains("شركة الإمارات للطاقة"));
    assert!(s.contains("renewable mandates"));
}

#[test]
fn unknown_prompt_name_errors() {
    let lib = PromptLibrary::v1();
    assert!(lib
        .render("doesnotexist", &PromptContext::default())
        .is_err());
}

#[test]
fn schemas_are_objects() {
    assert!(PromptLibrary::navigate_schema().is_object());
    assert!(PromptLibrary::summarize_schema().is_object());
    assert!(PromptLibrary::keywords_schema().is_object());
}
