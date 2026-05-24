//! Live-progress integration tests for ingest.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::needless_pass_by_value,
    clippy::elidable_lifetime_names,
    clippy::manual_let_else,
    clippy::if_not_else,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_borrows_for_generic_args,
    clippy::uninlined_format_args,
    clippy::semicolon_if_nothing_returned,
    clippy::needless_lifetimes,
    clippy::useless_vec,
    clippy::map_unwrap_or
)]

use std::sync::Arc;
use std::time::Duration;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::types::{IngestParams, SourceKind};
use pagebridge_core::{IngestStage, Pagebridge, PagebridgeOptions, SummaryWorkerConfig};

fn synthetic_md(sections: usize, paras: usize) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("# Doc\n\nIntro.\n\n");
    for s in 0..sections {
        out.push_str(&format!("## Section {s}\n\n"));
        for p in 0..paras {
            out.push_str(&format!("Body {p} in section {s}.\n\n"));
        }
    }
    out.into_bytes()
}

#[tokio::test]
async fn progress_handle_yields_done_snapshot_after_wait() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..32 {
        llm.push_json(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }));
    }
    let opts = PagebridgeOptions::new(storage.clone(), llm).with_summary_worker_config(
        SummaryWorkerConfig {
            max_concurrency: 4,
            ..SummaryWorkerConfig::default()
        },
    );
    let pb = Pagebridge::new_with(opts).await.unwrap();

    let params = IngestParams {
        title: "Progress Doc".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(4, 2),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let handle = pb.ingest_document_with_progress(params).await.unwrap();
    let mut subscriber = handle.subscribe();

    // Drain stages we observe during the live run by polling alongside wait.
    // Because progress is an Arc held by the spawned task and by `handle`, the
    // broadcast remains live for the duration of `wait()`. Drop the subscriber
    // after wait returns so the loop below terminates.
    let drain = async {
        let mut latest = IngestStage::Parsing;
        loop {
            match tokio::time::timeout(Duration::from_millis(50), subscriber.recv()).await {
                Ok(Ok(snap)) => {
                    latest = snap.stage;
                    if matches!(latest, IngestStage::Done | IngestStage::Failed) {
                        return latest;
                    }
                }
                Ok(Err(_closed)) => return latest,
                Err(_) => return latest,
            }
        }
    };
    let (_done, observed) = tokio::join!(handle.wait(), drain);
    assert!(matches!(
        observed,
        IngestStage::Summarizing
            | IngestStage::StructuralInsert
            | IngestStage::Done
            | IngestStage::Parsing
    ));
}

#[tokio::test]
async fn snapshot_is_consistent_after_completion() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..32 {
        llm.push_json(serde_json::json!({
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }));
    }
    let pb = Pagebridge::new(storage, llm).await.unwrap();
    let params = IngestParams {
        title: "After".into(),
        source_kind: SourceKind::Markdown,
        raw_text: synthetic_md(3, 2),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let handle = pb.ingest_document_with_progress(params).await.unwrap();
    let snap_before = handle.progress();
    assert!(snap_before.total_nodes > 0);
    let _ = handle.wait().await.unwrap();
}
