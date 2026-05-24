//! Eval input/output schema.

use serde::{Deserialize, Serialize};

/// A single question with ground-truth answer + citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalQuestion {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub ground_truth_answer: String,
    #[serde(default)]
    pub ground_truth_citations: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A full eval set: name, corpus paths, questions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSet {
    pub name: String,
    #[serde(default)]
    pub corpus: Vec<String>,
    pub questions: Vec<EvalQuestion>,
}

/// Per-question result row, also the shape written to CSV.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub question_id: String,
    pub retrieval_recall_at_1: f32,
    pub retrieval_recall_at_3: f32,
    pub retrieval_recall_at_5: f32,
    pub citation_precision: f32,
    pub bleu_lite: f32,
    pub latency_ms: u64,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

/// Aggregate summary across an entire run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    pub questions: u32,
    pub mean_recall_at_1: f32,
    pub mean_recall_at_3: f32,
    pub mean_recall_at_5: f32,
    pub mean_citation_precision: f32,
    pub mean_bleu_lite: f32,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
}

impl EvalSummary {
    /// Compute the summary from a slice of per-question results.
    #[must_use]
    pub fn from_results(results: &[EvalResult]) -> Self {
        let n = results.len();
        if n == 0 {
            return Self {
                questions: 0,
                mean_recall_at_1: 0.0,
                mean_recall_at_3: 0.0,
                mean_recall_at_5: 0.0,
                mean_citation_precision: 0.0,
                mean_bleu_lite: 0.0,
                p50_latency_ms: 0,
                p95_latency_ms: 0,
                p99_latency_ms: 0,
                total_tokens_in: 0,
                total_tokens_out: 0,
            };
        }
        let nf = n as f32;
        let sum = |f: fn(&EvalResult) -> f32| results.iter().map(f).sum::<f32>() / nf;
        let mut latencies: Vec<u64> = results.iter().map(|r| r.latency_ms).collect();
        latencies.sort_unstable();
        let pick = |q: f64| -> u64 {
            let idx = ((latencies.len() as f64) * q).floor() as usize;
            latencies[idx.min(latencies.len() - 1)]
        };
        let total_in: u64 = results.iter().map(|r| u64::from(r.tokens_in)).sum();
        let total_out: u64 = results.iter().map(|r| u64::from(r.tokens_out)).sum();
        Self {
            questions: u32::try_from(n).unwrap_or(u32::MAX),
            mean_recall_at_1: sum(|r| r.retrieval_recall_at_1),
            mean_recall_at_3: sum(|r| r.retrieval_recall_at_3),
            mean_recall_at_5: sum(|r| r.retrieval_recall_at_5),
            mean_citation_precision: sum(|r| r.citation_precision),
            mean_bleu_lite: sum(|r| r.bleu_lite),
            p50_latency_ms: pick(0.50),
            p95_latency_ms: pick(0.95),
            p99_latency_ms: pick(0.99),
            total_tokens_in: total_in,
            total_tokens_out: total_out,
        }
    }
}
