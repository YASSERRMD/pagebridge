//! Phase J4: type-aware structural parser dispatch.
//!
//! The generic parsers (`plain.rs`, `markdown.rs`, `pdf.rs`) produce a valid
//! node tree for any document but treat every input identically. When the
//! document type is known (e.g. a resume or research paper) the parser can
//! apply type-specific heuristics to find section boundaries more reliably,
//! especially in plain-text inputs that lack explicit headings.
//!
//! ## Dispatch rules
//!
//! | Source kind | Doc type known? | Parser used                        |
//! |-------------|----------------|------------------------------------|
//! | Markdown    | any            | generic markdown (headings work)   |
//! | PDF         | any            | generic PDF (page-based)           |
//! | Plain       | yes            | schema-guided heading detection    |
//! | Plain       | no / Generic   | generic plain-text chunker         |
//!
//! For Plain text the schema-guided parser looks for lines that match a
//! section alias from the document schema (case-insensitive) or that are
//! written in ALL CAPS. When a match is found it opens a new `Section` node;
//! content in between becomes `Leaf` nodes under that section.
//!
//! If the schema-guided parser finds fewer than 2 section boundaries it falls
//! back to the generic plain parser to avoid producing a degenerate tree.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value,
    clippy::too_many_arguments
)]

use crate::error::Result;
use crate::id::{DocId, NodeId};
use crate::ingest::{make_leaf, now_ms};
use crate::record::{NodeLevel, NodeRecord};
use crate::section_schema::{document_schema_for, SectionSchema};
use crate::types::{DocumentType, SourceKind};

/// Parse a document using a type-aware dispatcher.
///
/// When the document type is known and the source is plain text, a
/// schema-guided heading detector is applied first. If it finds fewer than
/// two heading boundaries, the result is discarded and the generic plain
/// chunker is used instead. Markdown and PDF inputs always use their generic
/// parsers because those already produce well-structured trees.
pub fn parse_typed(
    doc_id: &DocId,
    title: &str,
    doc_type: Option<DocumentType>,
    source_kind: SourceKind,
    text: &str,
    raw_bytes: &[u8],
) -> Result<Vec<NodeRecord>> {
    use crate::ingest::{markdown, pdf, plain};
    match source_kind {
        // Markdown headings already give reliable structure; type awareness
        // adds nothing here beyond the J3 tagging that happens in ingest_full.
        SourceKind::Markdown => markdown::parse(doc_id, title, text),
        // PDF is page-based; type-specific re-parsing is out of scope.
        SourceKind::Pdf => pdf::parse_bytes(doc_id, title, raw_bytes),
        SourceKind::Plain => {
            // Only apply schema guidance for well-defined non-Generic types.
            let try_typed = doc_type
                .filter(|dt| *dt != DocumentType::Generic)
                .map(document_schema_for);

            if let Some(schema) = try_typed {
                let typed = parse_plain_typed(doc_id, title, text, schema.sections)?;
                if count_sections(&typed) >= 2 {
                    return Ok(typed);
                }
                // Fall through to the generic parser if too few sections found.
            }
            plain::parse(doc_id, title, text)
        }
    }
}

/// Count Section-level nodes in a node list.
fn count_sections(nodes: &[NodeRecord]) -> usize {
    nodes
        .iter()
        .filter(|n| n.level == NodeLevel::Section)
        .count()
}

/// Parse plain text with schema-guided section detection.
///
/// A line is treated as a section heading when:
/// - It matches a schema alias (case-insensitive, after trimming), OR
/// - It is written entirely in ASCII uppercase and is non-empty.
///
/// Lines that look like headings open a new section; all other lines
/// accumulate into sentence-chunked leaves under the current section.
fn parse_plain_typed(
    doc_id: &DocId,
    title: &str,
    text: &str,
    sections: &'static [SectionSchema],
) -> Result<Vec<NodeRecord>> {
    let root_id = NodeId::root(doc_id);
    let mut nodes = Vec::new();
    nodes.push(NodeRecord {
        node_id: root_id.clone(),
        doc_id: doc_id.clone(),
        parent_id: None,
        title: title.to_owned(),
        level: NodeLevel::Document,
        routing_summary: String::new(),
        summary: String::new(),
        child_ids: vec![],
        span: None,
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: false,
        created_at: now_ms(),
        updated_at: now_ms(),
        source_hash: [0; 32],
        canonical_section: None,
        section_aliases: vec![],
    });

    // Segment the text into blocks: each block is (heading_opt, body_text).
    let blocks = split_into_heading_blocks(text, sections);

    let mut sec_seq: u32 = 0;
    let mut leaf_seq: u32 = 0;

    for (heading, body) in blocks {
        let (section_title, canonical, aliases) = heading
            .map(|h| {
                let canon = h.canonical_name.to_owned();
                let al: Vec<String> = h.aliases.iter().map(|a| (*a).to_owned()).collect();
                (h.canonical_name.to_owned(), Some(canon), al)
            })
            .unwrap_or_else(|| ("content".to_owned(), None, vec![]));

        sec_seq += 1;
        let sec_id = root_id.child("sec", &sec_seq.to_string())?;
        nodes.push(NodeRecord {
            node_id: sec_id.clone(),
            doc_id: doc_id.clone(),
            parent_id: Some(root_id.clone()),
            title: section_title,
            level: NodeLevel::Section,
            routing_summary: String::new(),
            summary: String::new(),
            child_ids: vec![],
            span: None,
            page_start: None,
            page_end: None,
            keywords: vec![],
            is_leaf: false,
            created_at: now_ms(),
            updated_at: now_ms(),
            source_hash: [0; 32],
            canonical_section: canonical,
            section_aliases: aliases,
        });

        // Chunk the body into leaves (max 10 sentences per leaf).
        let sentences = split_sentences(body);
        for (chunk_idx, chunk) in sentences.chunks(10).enumerate() {
            if chunk.is_empty() {
                continue;
            }
            leaf_seq += 1;
            let (start, _, _) = chunk[0];
            let (_, end, _) = chunk[chunk.len() - 1];
            let preview = truncate(chunk[0].2, 120);
            let _ = chunk_idx; // suppress lint
            let leaf = make_leaf(
                doc_id,
                &sec_id,
                leaf_seq,
                preview.clone(),
                (start as u64, end as u64),
                None,
                None,
                preview,
            )?;
            nodes.push(leaf);
        }
    }
    Ok(nodes)
}

/// Split text into `(Option<&SectionSchema>, body_str)` blocks. Each block
/// starts at a detected heading (or at the start of text for the first block)
/// and continues until the next heading.
fn split_into_heading_blocks<'a>(
    text: &'a str,
    sections: &'static [SectionSchema],
) -> Vec<(Option<&'static SectionSchema>, &'a str)> {
    let mut blocks: Vec<(Option<&'static SectionSchema>, &'a str)> = Vec::new();
    let mut current_schema: Option<&'static SectionSchema> = None;
    let mut body_start: usize = 0;

    let mut byte_offset: usize = 0;
    let lines: Vec<(usize, &str)> = text
        .lines()
        .map(|l| {
            let off = byte_offset;
            byte_offset += l.len() + 1; // +1 for '\n'
            (off, l)
        })
        .collect();

    for (off, line) in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(sec) = detect_heading(trimmed, sections) {
            // Close previous block.
            let body = &text[body_start..*off];
            if !body.trim().is_empty() || !blocks.is_empty() {
                blocks.push((current_schema, body));
            }
            current_schema = Some(sec);
            body_start = off + line.len() + 1;
        }
    }
    // Final block.
    let tail = &text[body_start..];
    if !tail.trim().is_empty() || !blocks.is_empty() {
        blocks.push((current_schema, tail));
    }
    blocks
}

/// Detect whether `line` (already trimmed) is a section heading.
/// Returns the matched schema entry, or `None` if no heading detected.
fn detect_heading(
    line: &str,
    sections: &'static [SectionSchema],
) -> Option<&'static SectionSchema> {
    // Short lines only (headings rarely exceed 60 chars).
    if line.len() > 60 {
        return None;
    }
    // Schema-alias match takes priority.
    if let Some(sec) = sections.iter().find(|s| s.matches(line)) {
        return Some(sec);
    }
    // ALL-CAPS line that looks like a heading (at least 3 chars, no digits-only).
    let stripped = line.trim_end_matches(':');
    if stripped.len() >= 3
        && stripped
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == ' ' || c == '-' || c == '/')
        && stripped.chars().any(|c| c.is_ascii_alphabetic())
    {
        // Try matching the all-caps version against schema too.
        return sections.iter().find(|s| s.matches(stripped));
    }
    None
}

/// Crude sentence split on `. `, `! `, `? `, and newlines.
fn split_sentences(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let here = bytes[i];
        let break_here = matches!(here, b'\n') || {
            i + 1 < bytes.len()
                && (here == b'.' || here == b'!' || here == b'?')
                && matches!(bytes[i + 1], b' ' | b'\n')
        };
        if break_here {
            let end = i + 1;
            let segment = text[start..end].trim();
            if !segment.is_empty() {
                out.push((start, end, &text[start..end]));
            }
            start = end;
        }
        i += 1;
    }
    if start < text.len() {
        let segment = text[start..].trim();
        if !segment.is_empty() {
            out.push((start, text.len(), &text[start..]));
        }
    }
    out
}

/// Truncate to `max` characters.
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section_schema::document_schema_for;
    use crate::types::DocumentType;

    fn doc_id() -> DocId {
        DocId::new("test-typed-abc123".to_owned()).unwrap()
    }

    #[test]
    fn parse_typed_plain_resume_detects_sections() {
        let text =
            "skills\nRust, Python.\n\nexperience\nAcme Corp 2020-2024.\n\neducation\nBSc CS.\n";
        let doc = doc_id();
        let nodes = parse_typed(
            &doc,
            "My CV",
            Some(DocumentType::Resume),
            SourceKind::Plain,
            text,
            &[],
        )
        .unwrap();
        let sections: Vec<_> = nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Section)
            .collect();
        assert!(
            sections.len() >= 2,
            "should detect at least 2 sections, got {}",
            sections.len()
        );
        let has_skills = sections
            .iter()
            .any(|n| n.canonical_section.as_deref() == Some("skills"));
        assert!(has_skills, "skills section must be detected");
    }

    #[test]
    fn parse_typed_falls_back_when_too_few_sections() {
        // Only one recognizable heading -> fall back to generic plain parser.
        let text = "Some random text. No clear sections. Just prose.";
        let doc = doc_id();
        let nodes = parse_typed(
            &doc,
            "Generic Doc",
            Some(DocumentType::Resume),
            SourceKind::Plain,
            text,
            &[],
        )
        .unwrap();
        // Generic plain parser should still produce at least a root node.
        assert!(!nodes.is_empty());
    }

    #[test]
    fn parse_typed_markdown_delegates_to_generic() {
        let text = "# Skills\nRust.\n\n# Experience\nAcme.\n";
        let doc = doc_id();
        let nodes = parse_typed(
            &doc,
            "CV",
            Some(DocumentType::Resume),
            SourceKind::Markdown,
            text,
            &[],
        )
        .unwrap();
        assert!(!nodes.is_empty());
    }

    #[test]
    fn detect_heading_schema_alias() {
        let schema = document_schema_for(DocumentType::Resume);
        let sec = detect_heading("Work Experience", schema.sections);
        assert_eq!(sec.map(|s| s.canonical_name), Some("experience"));
    }

    #[test]
    fn detect_heading_all_caps_schema_match() {
        let schema = document_schema_for(DocumentType::Resume);
        let sec = detect_heading("SKILLS", schema.sections);
        assert_eq!(sec.map(|s| s.canonical_name), Some("skills"));
    }

    #[test]
    fn detect_heading_long_line_returns_none() {
        let schema = document_schema_for(DocumentType::Resume);
        let long = "a".repeat(65);
        assert!(detect_heading(&long, schema.sections).is_none());
    }

    #[test]
    fn count_sections_counts_correctly() {
        let schema = document_schema_for(DocumentType::Resume);
        let text = "skills\nRust.\n\nexperience\nAcme.\n\neducation\nBSc.\n";
        let doc = doc_id();
        let nodes = parse_plain_typed(&doc, "Test", text, schema.sections).unwrap();
        assert!(
            count_sections(&nodes) >= 2,
            "expected >= 2 sections, got {}",
            count_sections(&nodes)
        );
    }
}
