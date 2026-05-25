//! Plain-text chunking. Splits into sentences, groups into leaves and sections.

use crate::error::Result;
use crate::id::{DocId, NodeId};
use crate::ingest::{make_leaf, now_ms};
use crate::record::{NodeLevel, NodeRecord};

const LEAVES_PER_SECTION: usize = 5;
const SENTENCES_PER_LEAF: usize = 10;

/// Parse plain text into a NodeRecord tree.
pub fn parse(doc_id: &DocId, title: &str, text: &str) -> Result<Vec<NodeRecord>> {
    let mut nodes = Vec::new();
    let root_id = NodeId::root(doc_id);
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

    let sentences = split_sentences(text);
    if sentences.is_empty() {
        return Ok(nodes);
    }

    // Group sentences -> leaves.
    let leaf_chunks: Vec<Vec<(usize, usize, &str)>> = sentences
        .chunks(SENTENCES_PER_LEAF)
        .map(<[_]>::to_vec)
        .collect();
    // Group leaves -> sections.
    let section_chunks: Vec<&[Vec<(usize, usize, &str)>]> =
        leaf_chunks.chunks(LEAVES_PER_SECTION).collect();

    let mut leaf_seq: u32 = 0;
    for (sec_idx, sec_chunks) in section_chunks.iter().enumerate() {
        let sec_no = (sec_idx + 1) as u32;
        let sec_id = root_id.child("sec", &sec_no.to_string())?;
        let first_sentence = sec_chunks
            .first()
            .and_then(|c| c.first())
            .map_or("", |(_, _, s)| *s);
        let title_preview = preview(first_sentence, 80);
        nodes.push(NodeRecord {
            node_id: sec_id.clone(),
            doc_id: doc_id.clone(),
            parent_id: Some(root_id.clone()),
            title: format!("Section {sec_no}: {title_preview}"),
            level: NodeLevel::Section,
            routing_summary: title_preview,
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
        for leaf in sec_chunks.iter() {
            if leaf.is_empty() {
                continue;
            }
            leaf_seq += 1;
            let (start, _, _) = leaf[0];
            let (_, end, _) = leaf[leaf.len() - 1];
            let preview_text = preview(leaf[0].2, 120);
            let n = make_leaf(
                doc_id,
                &sec_id,
                leaf_seq,
                preview_text.clone(),
                (start as u64, end as u64),
                None,
                None,
                preview_text,
            )?;
            nodes.push(n);
        }
    }
    Ok(nodes)
}

/// Crude sentence split: on `. ` `! ` `? ` and newlines.
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
                out.push((start, end, segment_in_text(text, start, end)));
            }
            start = end;
        }
        i += 1;
    }
    if start < text.len() {
        let segment = text[start..].trim();
        if !segment.is_empty() {
            out.push((start, text.len(), segment_in_text(text, start, text.len())));
        }
    }
    out
}

fn segment_in_text<'a>(text: &'a str, start: usize, end: usize) -> &'a str {
    text[start..end].trim_matches(|c: char| c == ' ' || c == '\n' || c == '\t')
}

fn preview(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max.min(s.len()));
    for ch in s.chars().take(max) {
        out.push(ch);
    }
    out
}
