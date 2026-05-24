//! Tests for `Pagebridge::update_document`.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{DiffMode, IngestParams, Pagebridge, SourceKind, UpdateParams};

async fn make_bridge() -> Pagebridge {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    for _ in 0..60 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    Pagebridge::new(storage, echo).await.unwrap()
}

#[tokio::test]
async fn replace_swaps_document_under_same_id() {
    let bridge = make_bridge().await;
    let initial = bridge
        .ingest_document(IngestParams {
            title: "Carbon".into(),
            source_kind: SourceKind::Markdown,
            raw_text: b"# Carbon\n\n## V1\n\noriginal body\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&initial.doc_id).await.unwrap();

    let report = bridge
        .update_document(UpdateParams {
            doc_id: initial.doc_id.clone(),
            new_raw_text: b"# Carbon\n\n## V2\n\nnew body\n".to_vec(),
            diff_mode: DiffMode::Replace,
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&initial.doc_id).await.unwrap();

    assert_eq!(report.doc_id, initial.doc_id);
    assert!(report.new_leaves > 0 || report.unchanged_leaves > 0);
    let docs = bridge.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
}

#[tokio::test]
async fn incremental_mode_falls_back_to_replace_for_now() {
    let bridge = make_bridge().await;
    let initial = bridge
        .ingest_document(IngestParams {
            title: "Doc".into(),
            source_kind: SourceKind::Markdown,
            raw_text: b"# D\n\n## A\n\nbody\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&initial.doc_id).await.unwrap();
    let res = bridge
        .update_document(UpdateParams {
            doc_id: initial.doc_id.clone(),
            new_raw_text: b"# D\n\n## A\n\nbody\n\n## B\n\nnew section\n".to_vec(),
            diff_mode: DiffMode::Incremental,
        })
        .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn append_only_reports_zero_changes() {
    let bridge = make_bridge().await;
    let initial = bridge
        .ingest_document(IngestParams {
            title: "Log".into(),
            source_kind: SourceKind::Plain,
            raw_text: b"line one\nline two\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&initial.doc_id).await.unwrap();
    let report = bridge
        .update_document(UpdateParams {
            doc_id: initial.doc_id.clone(),
            new_raw_text: b"line three\n".to_vec(),
            diff_mode: DiffMode::AppendOnly,
        })
        .await
        .unwrap();
    assert_eq!(report.removed_leaves, 0);
    assert_eq!(report.new_leaves, 0);
    assert_eq!(report.changed_leaves, 0);
    assert!(report.unchanged_leaves > 0);
}
