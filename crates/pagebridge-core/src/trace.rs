//! Trace builder threaded through the navigation and synthesis passes.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::format_push_string,
    clippy::missing_errors_doc
)]

use crate::id::NodeId;
use crate::ingest;
use crate::types::{Citation, QueryTrace, SearchHit, TraceStep};

/// Helper that accumulates trace steps during a query. Owned by the public
/// facade (`Pagebridge::ask`) and passed mutably into the navigator and the
/// synthesizer.
#[derive(Debug, Clone)]
pub struct TraceBuilder {
    pub data: QueryTrace,
}

impl TraceBuilder {
    /// New empty trace tagged with a fresh query id.
    #[must_use]
    pub fn new(question: &str) -> Self {
        let started = ingest::now_ms();
        Self {
            data: QueryTrace {
                query_id: format!("q-{started}"),
                question: question.to_owned(),
                started_at: started,
                finished_at: started,
                duration_ms: 0,
                steps: Vec::new(),
                total_llm_calls: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                bm25_candidates: Vec::new(),
                selected_leaves: Vec::new(),
                final_citations: Vec::new(),
            },
        }
    }

    pub fn bm25(&mut self, hits: &[SearchHit], top_score: f32) {
        self.data.bm25_candidates = hits.to_vec();
        self.data.steps.push(TraceStep::Bm25Candidates {
            count: hits.len(),
            top_score,
        });
    }

    pub fn nav_decision(
        &mut self,
        node_id: NodeId,
        action: String,
        reason: Option<String>,
        input_tokens: u32,
        output_tokens: u32,
        duration_ms: u64,
    ) {
        self.data.total_llm_calls += 1;
        self.data.total_input_tokens += input_tokens;
        self.data.total_output_tokens += output_tokens;
        self.data.steps.push(TraceStep::NavigationDecision {
            node_id,
            action,
            reason,
            input_tokens,
            output_tokens,
            duration_ms,
        });
    }

    pub fn leaf_selection(&mut self, leaves: &[NodeId]) {
        self.data.selected_leaves = leaves.to_vec();
        self.data.steps.push(TraceStep::LeafSelection {
            leaves: leaves.to_vec(),
        });
    }

    pub fn synthesis_start(&mut self, leaf_count: usize, total_chars: usize) {
        self.data.steps.push(TraceStep::SynthesisStart {
            leaf_count,
            total_chars,
        });
    }

    pub fn synthesis_done(&mut self, input_tokens: u32, output_tokens: u32, duration_ms: u64) {
        self.data.total_llm_calls += 1;
        self.data.total_input_tokens += input_tokens;
        self.data.total_output_tokens += output_tokens;
        self.data.steps.push(TraceStep::SynthesisDone {
            input_tokens,
            output_tokens,
            duration_ms,
        });
    }

    pub fn final_citations(&mut self, citations: &[NodeId]) {
        self.data.final_citations = citations.to_vec();
    }

    pub fn budget_exhausted(&mut self, reason: &str) {
        self.data.steps.push(TraceStep::BudgetExhausted {
            reason: reason.to_owned(),
        });
    }

    pub fn finish(&mut self) {
        self.data.finished_at = ingest::now_ms();
        self.data.duration_ms = self.data.finished_at.saturating_sub(self.data.started_at) as u64;
    }

    #[must_use]
    pub fn clone_data(&self) -> QueryTrace {
        let mut d = self.data.clone();
        d.finished_at = ingest::now_ms();
        d.duration_ms = d.finished_at.saturating_sub(d.started_at) as u64;
        d
    }
}

/// Render citations into a human-readable suffix that callers can append to
/// answers when they want a plain-text trail without programmatic citations.
#[must_use]
pub fn render_citation_list(citations: &[Citation]) -> String {
    let mut out = String::new();
    for (i, c) in citations.iter().enumerate() {
        out.push_str(&format!(
            "[{i}] {} / {} ({})\n",
            c.doc_title,
            c.section_title,
            c.node_id.as_str()
        ));
        let _ = i;
    }
    out
}
