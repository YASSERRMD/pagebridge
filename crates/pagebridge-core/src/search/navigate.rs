//! LLM-guided tree navigation.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use crate::adapter::StorageAdapter;
use crate::error::{PagebridgeError, Result};
use crate::id::{DocId, NodeId};
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider};
use crate::prompts::{PromptContext, PromptLibrary};
use crate::record::NodeRecord;
use crate::section_schema::document_schema_for;
use crate::trace::TraceBuilder;
use crate::types::{DocumentType, NavigationConfig, SearchHit};

use super::candidates;
use super::query_intent;

/// Outcome of a navigation pass: the chosen leaf records.
#[derive(Debug, Clone)]
pub struct NavigationOutcome {
    pub selected_leaves: Vec<NodeRecord>,
    pub bm25_candidates: Vec<SearchHit>,
    pub considered_roots: Vec<NodeId>,
}

/// Decide which leaves are relevant by combining BM25 candidates with
/// LLM-guided beam search.
pub async fn navigate(
    storage: &Arc<dyn StorageAdapter>,
    llm: &Arc<dyn LlmProvider>,
    prompts: &PromptLibrary,
    question: &str,
    cfg: NavigationConfig,
    doc_filter: Option<&DocId>,
    trace: &mut TraceBuilder,
) -> Result<NavigationOutcome> {
    // Fetch the document entry once; reused by J6 expansion and J7 fast-path.
    let doc_entry = match doc_filter {
        Some(did) => storage.get_document_entry(did).await?,
        None => None,
    };
    let doc_type: Option<DocumentType> = doc_entry.as_ref().and_then(|e| e.document_type);

    // Phase J6: expand the BM25 query with canonical section aliases when the
    // document type is known and non-Generic.
    let expanded_question: String = match doc_type {
        Some(dt) if dt != DocumentType::Generic => {
            query_intent::maybe_expand(question, dt, cfg.query_intent)
        }
        _ => question.to_owned(),
    };
    let bm25_query = expanded_question.as_str();

    // Phase J7: section-first fast path.
    // When query intent can be classified to a specific canonical section, jump
    // directly to that section's leaves without running BM25 or LLM navigation.
    if let (Some(did), Some(dt)) = (doc_filter, doc_type) {
        if dt != DocumentType::Generic && cfg.query_intent.enabled {
            let schema = document_schema_for(dt);
            if let Some(sec) = query_intent::classify_query_intent(question, dt, schema) {
                let section_leaves = storage
                    .find_nodes_by_canonical(did, sec.canonical_name)
                    .await?;
                if !section_leaves.is_empty() {
                    let selected: Vec<NodeRecord> = section_leaves
                        .into_iter()
                        .take(cfg.max_leaves as usize)
                        .collect();
                    trace.leaf_selection(
                        &selected
                            .iter()
                            .map(|n| n.node_id.clone())
                            .collect::<Vec<_>>(),
                    );
                    return Ok(NavigationOutcome {
                        selected_leaves: selected,
                        bm25_candidates: vec![],
                        considered_roots: vec![NodeId::root(did)],
                    });
                }
            }
        }
    }

    let bm25 =
        candidates::select(storage, bm25_query, cfg.bm25_candidate_limit, doc_filter).await?;
    let top_score = bm25.first().map_or(0.0, |h| h.score);
    trace.bm25(&bm25, top_score);

    if bm25.is_empty() {
        return Ok(NavigationOutcome {
            selected_leaves: Vec::new(),
            bm25_candidates: bm25,
            considered_roots: Vec::new(),
        });
    }

    // Build the list of document roots we will navigate from (top 5 by score).
    let mut considered_roots: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<DocId> = HashSet::new();
    for h in &bm25 {
        if seen.insert(h.doc_id.clone()) {
            considered_roots.push(NodeId::root(&h.doc_id));
            if considered_roots.len() >= 5 {
                break;
            }
        }
    }

    let mut selected: Vec<NodeRecord> = Vec::new();
    let mut llm_calls: u32 = 0;
    let mut leaf_seen: HashSet<NodeId> = HashSet::new();

    for root in &considered_roots {
        let outcome = walk_one_root(
            storage,
            llm,
            prompts,
            question,
            cfg,
            root,
            &mut llm_calls,
            trace,
        )
        .await?;
        for leaf in outcome {
            if leaf_seen.insert(leaf.node_id.clone()) {
                selected.push(leaf);
                if selected.len() >= cfg.max_leaves as usize {
                    break;
                }
            }
        }
        if selected.len() >= cfg.max_leaves as usize {
            break;
        }
        if llm_calls >= u32::from(cfg.max_llm_calls) {
            trace.budget_exhausted("max_llm_calls reached");
            break;
        }
    }

    // Fallback: if the LLM never converged, take the top BM25 leaves directly.
    if selected.is_empty() {
        for h in bm25.iter().take(cfg.max_leaves as usize) {
            if let Some(rec) = storage.get_node(&h.node_id).await? {
                if rec.is_leaf {
                    selected.push(rec);
                }
            }
        }
    }

    trace.leaf_selection(
        &selected
            .iter()
            .map(|n| n.node_id.clone())
            .collect::<Vec<_>>(),
    );

    Ok(NavigationOutcome {
        selected_leaves: selected,
        bm25_candidates: bm25,
        considered_roots,
    })
}

async fn walk_one_root(
    storage: &Arc<dyn StorageAdapter>,
    llm: &Arc<dyn LlmProvider>,
    prompts: &PromptLibrary,
    question: &str,
    cfg: NavigationConfig,
    root: &NodeId,
    llm_calls: &mut u32,
    trace: &mut TraceBuilder,
) -> Result<Vec<NodeRecord>> {
    let mut frontier: Vec<NodeId> = vec![root.clone()];
    let mut chosen_leaves: Vec<NodeRecord> = Vec::new();
    let mut current_path: Vec<crate::record::NodeSummary> = Vec::new();
    let mut depth: u8 = 0;

    while !frontier.is_empty() && depth < cfg.max_depth {
        let mut next_frontier: Vec<NodeId> = Vec::new();

        for current in frontier.iter().take(cfg.beam_width as usize) {
            if *llm_calls >= u32::from(cfg.max_llm_calls) {
                return Ok(chosen_leaves);
            }
            let children = storage.children_summaries(current).await?;
            if children.is_empty() {
                // Treat as a leaf candidate.
                if let Some(rec) = storage.get_node(current).await? {
                    if rec.is_leaf {
                        chosen_leaves.push(rec);
                    }
                }
                continue;
            }
            let doc_title = storage
                .get_node(&NodeId::root(&current.doc_id()?))
                .await?
                .map(|r| r.title)
                .unwrap_or_default();
            let ctx = PromptContext {
                question: Some(question.to_owned()),
                children: children.clone(),
                document_title: Some(doc_title),
                current_path: current_path.clone(),
                ..PromptContext::default()
            };
            let prompt = prompts.render("navigate", &ctx)?;
            let start = Instant::now();
            let resp = llm
                .complete_json(
                    CompletionRequest {
                        system: Some("You navigate a hierarchical retrieval index.".into()),
                        messages: vec![ChatMessage::user(prompt)],
                        ..CompletionRequest::default()
                    },
                    &PromptLibrary::navigate_schema(),
                )
                .await
                .unwrap_or_else(|_| serde_json::json!({"action": "select_leaves", "node_ids": []}));
            let elapsed = start.elapsed().as_millis() as u64;
            *llm_calls += 1;
            let action = resp
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("select_leaves")
                .to_owned();
            let reason = resp
                .get("reason")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            trace.nav_decision(current.clone(), action.clone(), reason, 0, 0, elapsed);

            match action.as_str() {
                "descend" => {
                    if let Some(target) = resp.get("node_id").and_then(|v| v.as_str()) {
                        if let Ok(target_id) = NodeId::new(target) {
                            if let Some(summary) =
                                children.iter().find(|c| c.node_id == target_id).cloned()
                            {
                                next_frontier.push(target_id);
                                current_path.push(summary);
                            }
                        }
                    }
                }
                "select_leaves" => {
                    if let Some(ids) = resp.get("node_ids").and_then(|v| v.as_array()) {
                        for v in ids {
                            if let Some(s) = v.as_str() {
                                if let Ok(id) = NodeId::new(s) {
                                    if let Some(rec) = storage.get_node(&id).await? {
                                        if rec.is_leaf {
                                            chosen_leaves.push(rec);
                                            if chosen_leaves.len() >= cfg.max_leaves as usize {
                                                return Ok(chosen_leaves);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                "bm25_fallback" => {
                    // Re-issue BM25 with the rewritten query and treat top leaves as selected.
                    let query = resp
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or(question);
                    let doc_id = current.doc_id().ok();
                    let hits = candidates::select(
                        storage,
                        query,
                        cfg.max_leaves as usize,
                        doc_id.as_ref(),
                    )
                    .await?;
                    for h in hits {
                        if let Some(rec) = storage.get_node(&h.node_id).await? {
                            if rec.is_leaf {
                                chosen_leaves.push(rec);
                                if chosen_leaves.len() >= cfg.max_leaves as usize {
                                    return Ok(chosen_leaves);
                                }
                            }
                        }
                    }
                }
                _ => {
                    // "widen" or unknown: just halt this branch.
                }
            }
        }

        frontier = next_frontier;
        depth += 1;
    }

    let _ = depth;
    let _ = PagebridgeError::Internal(String::new());
    Ok(chosen_leaves)
}
