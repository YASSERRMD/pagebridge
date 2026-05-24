//! Lightweight metric implementations used by [`crate::run`].
//!
//! Everything here is intentionally simple: zero external NLP deps so the
//! eval framework stays buildable on edge devices. For comparing across
//! research benchmarks, swap in stricter implementations.

use std::collections::HashSet;

/// Recall at k: fraction of ground-truth citations that appear in the top
/// `k` predicted citations.
#[must_use]
pub fn retrieval_recall_at_k(predicted: &[String], ground_truth: &[String], k: usize) -> f32 {
    if ground_truth.is_empty() {
        return 1.0;
    }
    let topk: HashSet<&str> = predicted.iter().take(k).map(String::as_str).collect();
    let hits = ground_truth
        .iter()
        .filter(|gt| topk.contains(gt.as_str()))
        .count() as f32;
    hits / ground_truth.len() as f32
}

/// Citation precision: fraction of predicted citations that appear in the
/// ground-truth set.
#[must_use]
pub fn citation_precision(predicted: &[String], ground_truth: &[String]) -> f32 {
    if predicted.is_empty() {
        return if ground_truth.is_empty() { 1.0 } else { 0.0 };
    }
    let gt: HashSet<&str> = ground_truth.iter().map(String::as_str).collect();
    let hits = predicted.iter().filter(|p| gt.contains(p.as_str())).count() as f32;
    hits / predicted.len() as f32
}

/// "BLEU lite": modified n-gram precision averaged over n=1..=4.
/// No brevity penalty, no smoothing. Useful as a fast proxy for answer
/// similarity in CI; not a replacement for real BLEU when reporting numbers.
#[must_use]
pub fn bleu_lite(prediction: &str, reference: &str) -> f32 {
    if reference.trim().is_empty() {
        return 0.0;
    }
    let pred_tokens: Vec<&str> = prediction.split_whitespace().collect();
    let ref_tokens: Vec<&str> = reference.split_whitespace().collect();
    if pred_tokens.is_empty() {
        return 0.0;
    }
    let mut total = 0.0_f32;
    let mut counted = 0u32;
    for n in 1..=4usize {
        let p = ngram_precision(&pred_tokens, &ref_tokens, n);
        if p > 0.0 {
            total += p;
            counted += 1;
        }
    }
    if counted == 0 {
        0.0
    } else {
        total / counted as f32
    }
}

fn ngram_precision(pred: &[&str], reference: &[&str], n: usize) -> f32 {
    if pred.len() < n || reference.len() < n {
        return 0.0;
    }
    let pred_grams: Vec<&[&str]> = pred.windows(n).collect();
    let ref_grams: std::collections::HashSet<Vec<&str>> =
        reference.windows(n).map(<[&str]>::to_vec).collect();
    let hits = pred_grams
        .iter()
        .filter(|g| ref_grams.contains(&g.to_vec()))
        .count();
    hits as f32 / pred_grams.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_full_when_all_predictions_match() {
        let pred = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let gt = vec!["a".to_string(), "b".to_string()];
        assert!((retrieval_recall_at_k(&pred, &gt, 3) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recall_partial_at_k1() {
        let pred = vec!["x".to_string(), "a".to_string()];
        let gt = vec!["a".to_string()];
        assert!((retrieval_recall_at_k(&pred, &gt, 1) - 0.0).abs() < 1e-6);
        assert!((retrieval_recall_at_k(&pred, &gt, 2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn precision_zero_when_no_matches() {
        let pred = vec!["x".to_string(), "y".to_string()];
        let gt = vec!["a".to_string()];
        assert!(citation_precision(&pred, &gt) < 1e-6);
    }

    #[test]
    fn bleu_lite_identical_strings_score_one() {
        let s = "the rollout is in Q1 with phased coverage";
        assert!((bleu_lite(s, s) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn bleu_lite_disjoint_strings_score_zero() {
        assert!(bleu_lite("alpha beta gamma", "x y z").abs() < 1e-3);
    }
}
