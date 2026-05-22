//! Extract citation node ids from a synthesized answer and turn them into
//! `Citation` records.

#![allow(
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::missing_const_for_fn,
    clippy::single_match_else,
    clippy::manual_pattern_char_comparison
)]

use std::sync::Arc;

use crate::adapter::StorageAdapter;
use crate::id::NodeId;
use crate::record::NodeRecord;
use crate::types::Citation;

/// Strip the `<citations>...</citations>` trailer and return the cleaned
/// answer text plus the list of cited node ids.
pub fn extract(answer: &str) -> (String, Vec<NodeId>) {
    let mut ids = Vec::new();
    let cleaned = if let Some(start) = answer.rfind("<citations>") {
        let after = &answer[start..];
        if let Some(end_rel) = after.find("</citations>") {
            let inner = &after[start_len("<citations>")..end_rel];
            for tok in inner.split(|c: char| c == ',' || c == ' ' || c == '\n') {
                let t = tok.trim();
                if t.is_empty() {
                    continue;
                }
                if let Ok(id) = NodeId::new(t) {
                    ids.push(id);
                }
            }
            answer[..start].trim_end().to_owned()
        } else {
            answer.to_owned()
        }
    } else {
        // Fallback: look for bracketed citations [doc:.../...] inline.
        for caps in inline_brackets(answer) {
            if let Ok(id) = NodeId::new(caps) {
                ids.push(id);
            }
        }
        answer.to_owned()
    };
    (cleaned, ids)
}

const fn start_len(s: &str) -> usize {
    s.len()
}

fn inline_brackets(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end_rel) = s[i + 1..].find(']') {
                let inner = &s[i + 1..i + 1 + end_rel];
                if inner.starts_with("doc:") {
                    out.push(inner.to_owned());
                }
                i += end_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Build `Citation` rows for the given cited ids, looking up titles and pages
/// from the storage adapter when available.
pub async fn build_citations(
    storage: &Arc<dyn StorageAdapter>,
    leaves: &[NodeRecord],
    cited_ids: &[NodeId],
) -> Vec<Citation> {
    let mut out = Vec::with_capacity(cited_ids.len());
    for id in cited_ids {
        let Some(leaf) = leaves.iter().find(|l| l.node_id == *id).cloned() else {
            continue;
        };
        let doc_root = NodeId::root(&leaf.doc_id);
        let doc_title = storage
            .get_node(&doc_root)
            .await
            .ok()
            .flatten()
            .map(|r| r.title)
            .unwrap_or_default();
        let excerpt: String = leaf.summary.chars().take(200).collect();
        out.push(Citation {
            node_id: leaf.node_id.clone(),
            doc_id: leaf.doc_id.clone(),
            doc_title,
            section_title: leaf.title.clone(),
            page_range: match (leaf.page_start, leaf.page_end) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            },
            excerpt,
        });
    }
    out
}
