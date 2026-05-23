//! End-to-end test of the eval runner against an in-memory bridge.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{IngestParams, Pagebridge, SourceKind};
use pagebridge_eval::runner::results_to_csv;
use pagebridge_eval::{run, EvalQuestion, EvalSet, EvalSummary};

#[tokio::test]
async fn end_to_end_eval_produces_results_and_summary() {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    // Heaps of canned summary + navigation JSON so ingest and ask never starve.
    for _ in 0..120 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let bridge = Pagebridge::new(storage, echo).await.unwrap();
    bridge
        .ingest_document(IngestParams {
            title: "Carbon".into(),
            source_kind: SourceKind::Markdown,
            raw_text: b"# Carbon\n\n## Timeline\n\nrollout in Q1\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();

    let set = EvalSet {
        name: "smoke".into(),
        corpus: vec!["carbon.md".into()],
        questions: vec![EvalQuestion {
            id: "q1".into(),
            question: "when is rollout?".into(),
            ground_truth_answer: "Q1".into(),
            ground_truth_citations: vec![],
            tags: vec![],
        }],
    };
    let results = run(&set, &bridge).await.unwrap();
    assert_eq!(results.len(), 1);
    // With no ground-truth citations, recall is defined as 1.0.
    assert!((results[0].retrieval_recall_at_1 - 1.0).abs() < 1e-6);

    let summary = EvalSummary::from_results(&results);
    assert_eq!(summary.questions, 1);

    let csv = results_to_csv(&results).unwrap();
    assert!(csv.contains("question_id"));
    assert!(csv.contains("q1"));
}
