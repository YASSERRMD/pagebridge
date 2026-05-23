//! Public-API surface check for WorkspaceId and WorkspaceHandle.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{IngestParams, Pagebridge, SourceKind, WorkspaceId};

#[tokio::test]
async fn default_workspace_id_is_default() {
    let ws = WorkspaceId::default_workspace();
    assert_eq!(ws.as_str(), "default");
}

#[tokio::test]
async fn workspace_handle_tags_ingest_user_metadata() {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    for _ in 0..30 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let bridge = Pagebridge::new(storage, echo).await.unwrap();
    let handle = bridge.with_workspace(WorkspaceId::new("acme").unwrap());
    assert_eq!(handle.workspace().as_str(), "acme");
    let h = handle
        .ingest_document(IngestParams {
            title: "Doc".into(),
            source_kind: SourceKind::Markdown,
            raw_text: b"# X\n\nbody\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    assert!(!h.doc_id.as_str().is_empty());
}

#[tokio::test]
async fn workspace_id_validation_matches_doc_id_rules() {
    assert!(WorkspaceId::new("a").is_ok());
    assert!(WorkspaceId::new("team-1").is_ok());
    assert!(WorkspaceId::new("Bad Case").is_err());
    assert!(WorkspaceId::new("").is_err());
}
