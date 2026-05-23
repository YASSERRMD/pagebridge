//! Reranker conformance suite (shared across every Reranker impl).
//!
//! Run with any boxed reranker; the suite asserts the invariants every
//! conformant implementation must satisfy:
//!
//! 1. The returned list has length <= top_k.
//! 2. Every index points back into the original docs slice.
//! 3. Scores are non-negative.
//! 4. With top_k=0, the result is empty.

use pagebridge_reranker::{stub::StubReranker, Reranker};

async fn check<R: Reranker>(r: &R) {
    let docs: Vec<String> = (0..6).map(|i| format!("doc-{i}")).collect();
    let out = r.rerank("q", &docs, 4).await.unwrap();
    assert!(out.len() <= 4);
    for d in &out {
        assert!(d.index < docs.len());
        assert!(d.score >= 0.0);
    }

    let empty = r.rerank("q", &docs, 0).await.unwrap();
    assert!(empty.is_empty());
}

#[tokio::test]
async fn stub_reranker_conforms() {
    check(&StubReranker).await;
}
