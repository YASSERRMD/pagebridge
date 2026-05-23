//! Eval runner: walks every question in an `EvalSet`, runs `Pagebridge::ask`,
//! and computes per-question metrics.

use std::time::Instant;

use pagebridge_core::error::Result;
use pagebridge_core::Pagebridge;

use crate::metrics::{bleu_lite, citation_precision, retrieval_recall_at_k};
use crate::schema::{EvalQuestion, EvalResult, EvalSet};

/// Run an `EvalSet` against a (pre-ingested) `Pagebridge`. The caller is
/// responsible for ingesting `set.corpus` first; the runner does not
/// auto-ingest because corpus loading is adapter-specific.
pub async fn run(set: &EvalSet, bridge: &Pagebridge) -> Result<Vec<EvalResult>> {
    let mut results = Vec::with_capacity(set.questions.len());
    for q in &set.questions {
        results.push(run_one(q, bridge).await?);
    }
    Ok(results)
}

async fn run_one(q: &EvalQuestion, bridge: &Pagebridge) -> Result<EvalResult> {
    let start = Instant::now();
    let answer = bridge.ask(&q.question).await?;
    let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let predicted: Vec<String> = answer
        .citations
        .iter()
        .map(|c| c.node_id.as_str().to_owned())
        .collect();
    Ok(EvalResult {
        question_id: q.id.clone(),
        retrieval_recall_at_1: retrieval_recall_at_k(&predicted, &q.ground_truth_citations, 1),
        retrieval_recall_at_3: retrieval_recall_at_k(&predicted, &q.ground_truth_citations, 3),
        retrieval_recall_at_5: retrieval_recall_at_k(&predicted, &q.ground_truth_citations, 5),
        citation_precision: citation_precision(&predicted, &q.ground_truth_citations),
        bleu_lite: bleu_lite(&answer.text, &q.ground_truth_answer),
        latency_ms: elapsed,
        tokens_in: answer.trace.total_input_tokens,
        tokens_out: answer.trace.total_output_tokens,
    })
}

/// Serialize per-question results as CSV. Header row included.
pub fn results_to_csv(results: &[EvalResult]) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    for r in results {
        wtr.serialize(r).map_err(|e| {
            pagebridge_core::error::PagebridgeError::Internal(format!("csv: {e}"))
        })?;
    }
    let bytes = wtr.into_inner().map_err(|e| {
        pagebridge_core::error::PagebridgeError::Internal(format!("csv: {e}"))
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
