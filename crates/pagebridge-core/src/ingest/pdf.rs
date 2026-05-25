//! PDF ingestion via the `pdf-extract` crate.
//!
//! Extracts text, treats each form-feed-separated chunk as a page, and chunks
//! each page into leaves the same way as plain text.
//!
//! Phase J9 adds an optional `try_extract_page_image` helper. When a
//! rasterization library is available (e.g. pdfium-render), replace the stub
//! body with real rendering code. The rest of the pipeline never depends on
//! the function returning `Some`, so the stub is safe forever.

use crate::error::{PagebridgeError, Result};
use crate::id::{DocId, NodeId};
use crate::ingest::{make_leaf, now_ms};
use crate::llm::VisionImage;
use crate::record::{NodeLevel, NodeRecord};

const SENTENCES_PER_LEAF: usize = 10;

/// Phase J9: attempt to rasterize the first page of a PDF to a PNG image.
///
/// This is a **stub** implementation that always returns `None`.  When a
/// production rasterizer (e.g. `pdfium-render`) is integrated, replace the
/// body with the actual rendering logic.  The surrounding pipeline treats
/// `None` as "no image available" and falls back to text-only classification,
/// so the stub is safe in production at any time.
///
/// The expected return value is a [`VisionImage`] with:
/// - `bytes`: raw PNG bytes of the first page at a moderate resolution
///   (e.g. 150 DPI, capped at ~1024 px wide).
/// - `media_type`: `"image/png"`.
#[must_use]
pub fn try_extract_page_image(_bytes: &[u8]) -> Option<VisionImage> {
    // Stub: real rasterizer goes here.
    None
}

/// Parse a PDF byte buffer into a tree.
pub fn parse_bytes(doc_id: &DocId, title: &str, bytes: &[u8]) -> Result<Vec<NodeRecord>> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|e| PagebridgeError::Parse {
        source_kind: "pdf".into(),
        message: e.to_string(),
    })?;
    let pages: Vec<&str> = text.split('\u{000c}').collect();

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

    let mut leaf_seq: u32 = 0;
    let mut byte_cursor: u64 = 0;
    for (i, page_text) in pages.iter().enumerate() {
        let page_no = (i + 1) as u32;
        if page_text.trim().is_empty() {
            byte_cursor += page_text.len() as u64 + 1;
            continue;
        }
        let sec_id = root_id.child("page", &page_no.to_string())?;
        let page_preview = preview(page_text, 80);
        nodes.push(NodeRecord {
            node_id: sec_id.clone(),
            doc_id: doc_id.clone(),
            parent_id: Some(root_id.clone()),
            title: format!("Page {page_no}: {page_preview}"),
            level: NodeLevel::Page,
            routing_summary: page_preview,
            summary: String::new(),
            child_ids: vec![],
            span: None,
            page_start: Some(page_no),
            page_end: Some(page_no),
            keywords: vec![],
            is_leaf: false,
            created_at: now_ms(),
            updated_at: now_ms(),
            source_hash: [0; 32],
            canonical_section: None,
            section_aliases: vec![],
        });

        // Chunk page into leaves.
        let sentences = split_sentences(page_text);
        for chunk in sentences.chunks(SENTENCES_PER_LEAF) {
            if chunk.is_empty() {
                continue;
            }
            leaf_seq += 1;
            let start = byte_cursor + chunk[0].0 as u64;
            let end = byte_cursor + chunk[chunk.len() - 1].1 as u64;
            let preview_text = preview(chunk[0].2, 120);
            let leaf = make_leaf(
                doc_id,
                &sec_id,
                leaf_seq,
                preview_text.clone(),
                (start, end),
                Some(page_no),
                Some(page_no),
                preview_text,
            )?;
            nodes.push(leaf);
        }
        byte_cursor += page_text.len() as u64 + 1; // include the form-feed
    }
    Ok(nodes)
}

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
                out.push((start, end, segment));
            }
            start = end;
        }
        i += 1;
    }
    if start < text.len() {
        let segment = text[start..].trim();
        if !segment.is_empty() {
            out.push((start, text.len(), segment));
        }
    }
    out
}

fn preview(s: &str, max: usize) -> String {
    s.chars().take(max).collect::<String>().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_extract_page_image_stub_returns_none() {
        // The stub must return None for any input until a real rasterizer is wired.
        assert!(try_extract_page_image(b"").is_none());
        assert!(try_extract_page_image(b"%PDF-1.4 fake").is_none());
    }
}
