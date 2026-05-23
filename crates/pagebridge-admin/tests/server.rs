//! In-process integration tests for the admin HTTP server.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_admin::router;
use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{IngestParams, Pagebridge, SourceKind};

async fn start_bridge() -> Pagebridge {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    for _ in 0..30 {
        echo.push_json(serde_json::json!({
            "title": "T", "routing_summary": "rs", "summary": "s", "keywords": []
        }));
    }
    let bridge = Pagebridge::new(storage, echo).await.unwrap();
    let handle = bridge
        .ingest_document(IngestParams {
            title: "Doc".into(),
            source_kind: SourceKind::Markdown,
            raw_text: b"# Doc\n\n## A\n\nrollout in Q1.\n".to_vec(),
            doc_id: None,
            user_metadata: Default::default(),
        })
        .await
        .unwrap();
    bridge.wait_for_summaries(&handle.doc_id).await.unwrap();
    bridge
}

#[tokio::test]
async fn admin_serves_index_and_api() {
    let bridge = start_bridge().await;
    let app = router(bridge);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    let health: serde_json::Value = client
        .get(format!("{base}/api/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");

    let docs: Vec<serde_json::Value> = client
        .get(format!("{base}/api/documents"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(docs.len(), 1);

    let index = client
        .get(&base)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(index.contains("pagebridge admin"));

    // /metrics endpoint should be Prometheus-shaped.
    let metrics = client
        .get(format!("{base}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains("pagebridge_"));
}
