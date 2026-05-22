//! Synthesis: turn selected leaves + question into a cited answer.

use std::sync::Arc;
use std::time::Instant;

use crate::adapter::StorageAdapter;
use crate::citation;
use crate::error::Result;
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider};
use crate::prompts::{PromptContext, PromptLibrary};
use crate::record::NodeRecord;
use crate::trace::TraceBuilder;
use crate::types::{Answer, Citation};

/// Read raw text for each selected leaf, build the synthesis prompt, call the
/// LLM, and parse out a final cited answer.
pub async fn synthesize_answer(
    storage: &Arc<dyn StorageAdapter>,
    llm: &Arc<dyn LlmProvider>,
    prompts: &PromptLibrary,
    question: &str,
    leaves: Vec<NodeRecord>,
    trace: &mut TraceBuilder,
) -> Result<Answer> {
    // Materialize leaf body text from the adapter when available.
    let mut hydrated = Vec::with_capacity(leaves.len());
    let mut total_chars = 0usize;
    for mut leaf in leaves {
        if let Some(span) = leaf.span {
            if let Ok(text) = storage.read_raw_text(&leaf.doc_id, span).await {
                if !text.trim().is_empty() {
                    leaf.summary = text;
                }
            }
        }
        total_chars += leaf.summary.chars().count();
        hydrated.push(leaf);
    }
    trace.synthesis_start(hydrated.len(), total_chars);

    if hydrated.is_empty() {
        let trace_data = trace.clone_data();
        return Ok(Answer {
            text: "No relevant content was found for that question.".to_owned(),
            citations: vec![],
            trace: trace_data,
        });
    }

    let ctx = PromptContext {
        question: Some(question.to_owned()),
        leaves: hydrated.clone(),
        ..PromptContext::default()
    };
    let prompt = prompts.render("synthesize", &ctx)?;
    let start = Instant::now();
    let resp = llm
        .complete(CompletionRequest {
            system: Some("Answer only using the provided leaves. Cite them by id.".into()),
            messages: vec![ChatMessage::user(prompt)],
            ..CompletionRequest::default()
        })
        .await?;
    let elapsed = start.elapsed().as_millis() as u64;
    trace.synthesis_done(resp.input_tokens, resp.output_tokens, elapsed);

    let (clean_text, cited_ids) = citation::extract(&resp.text);
    let citations: Vec<Citation> = citation::build_citations(storage, &hydrated, &cited_ids).await;
    trace.final_citations(
        &citations
            .iter()
            .map(|c| c.node_id.clone())
            .collect::<Vec<_>>(),
    );

    let trace_data = trace.clone_data();
    Ok(Answer {
        text: clean_text,
        citations,
        trace: trace_data,
    })
}
