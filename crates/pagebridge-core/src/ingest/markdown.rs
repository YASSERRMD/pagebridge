//! Minimal markdown heading parser. Splits on ATX-style `# ... ######` headings.

use crate::error::Result;
use crate::id::{DocId, NodeId};
use crate::ingest::{make_leaf, now_ms};
use crate::record::{NodeLevel, NodeRecord};

/// Parse markdown into a hierarchical NodeRecord tree.
pub fn parse(doc_id: &DocId, title: &str, text: &str) -> Result<Vec<NodeRecord>> {
    let mut nodes = Vec::new();
    let root = NodeRecord {
        node_id: NodeId::root(doc_id),
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
    };
    nodes.push(root);

    // Walk lines, accumulating section bodies, and emit nodes at heading boundaries.
    let mut stack: Vec<(u8, NodeId, u32)> = Vec::new(); // (heading level, node_id, child_counter)
    let mut leaf_seq: u32 = 0;
    let mut body_buf = String::new();
    let mut body_buf_start: u64 = 0;
    let root_node_id = NodeId::root(doc_id);

    let lines: Vec<&str> = text.lines().collect();
    let mut byte_cursor: u64 = 0;

    let flush_buf = |body_buf: &mut String,
                     body_buf_start: &mut u64,
                     current: &NodeId,
                     leaf_seq: &mut u32,
                     doc_id: &DocId,
                     nodes: &mut Vec<NodeRecord>|
     -> Result<()> {
        let trimmed = body_buf.trim();
        if !trimmed.is_empty() {
            let span = (*body_buf_start, *body_buf_start + body_buf.len() as u64);
            *leaf_seq += 1;
            let preview = preview(trimmed, 120);
            let leaf = make_leaf(
                doc_id,
                current,
                *leaf_seq,
                preview.clone(),
                span,
                None,
                None,
                preview,
            )?;
            nodes.push(leaf);
        }
        body_buf.clear();
        Ok(())
    };

    for line in &lines {
        let line_len = line.len() as u64 + 1; // include newline
        if let Some((lvl, htext)) = parse_heading(line) {
            // Flush any pending body under the current parent.
            let current = stack
                .last()
                .map_or(root_node_id.clone(), |(_, id, _)| id.clone());
            flush_buf(
                &mut body_buf,
                &mut body_buf_start,
                &current,
                &mut leaf_seq,
                doc_id,
                &mut nodes,
            )?;

            // Pop until we find a level strictly less than `lvl`.
            while let Some((top_lvl, _, _)) = stack.last() {
                if *top_lvl < lvl {
                    break;
                }
                stack.pop();
            }
            let parent = stack
                .last()
                .map_or(root_node_id.clone(), |(_, id, _)| id.clone());
            let seq = stack
                .last_mut()
                .map(|(_, _, c)| {
                    *c += 1;
                    *c
                })
                .unwrap_or_else(|| {
                    nodes
                        .iter()
                        .filter(|r| r.parent_id.as_ref() == Some(&root_node_id) && !r.is_leaf)
                        .count() as u32
                        + 1
                });
            let segment_value = seq.to_string();
            let id = parent.child("sec", &segment_value)?;
            let level_enum = match lvl {
                1 => NodeLevel::Section,
                2 => NodeLevel::Subsection,
                _ => NodeLevel::Page,
            };
            nodes.push(NodeRecord {
                node_id: id.clone(),
                doc_id: doc_id.clone(),
                parent_id: Some(parent),
                title: htext.clone(),
                level: level_enum,
                routing_summary: htext,
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
            });
            stack.push((lvl, id, 0));
            body_buf_start = byte_cursor + line_len;
            body_buf.clear();
        } else {
            body_buf.push_str(line);
            body_buf.push('\n');
        }
        byte_cursor += line_len;
    }

    let current = stack
        .last()
        .map_or(root_node_id.clone(), |(_, id, _)| id.clone());
    flush_buf(
        &mut body_buf,
        &mut body_buf_start,
        &current,
        &mut leaf_seq,
        doc_id,
        &mut nodes,
    )?;
    Ok(nodes)
}

fn parse_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut lvl = 0u8;
    for ch in trimmed.chars() {
        if ch == '#' {
            lvl += 1;
            if lvl > 6 {
                return None;
            }
        } else {
            break;
        }
    }
    if lvl == 0 {
        return None;
    }
    let after = &trimmed[lvl as usize..];
    if !after.starts_with(' ') && !after.starts_with('\t') {
        return None;
    }
    let title = after.trim().to_owned();
    if title.is_empty() {
        return None;
    }
    Some((lvl, title))
}

fn preview(s: &str, max: usize) -> String {
    let single_line: String = s.lines().next().unwrap_or("").chars().take(max).collect();
    if single_line.trim().is_empty() {
        s.chars().take(max).collect()
    } else {
        single_line
    }
}
