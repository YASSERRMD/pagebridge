//! Round-trip MCP JSON-RPC requests against an in-memory pagebridge.

#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::{IngestParams, Pagebridge, SourceKind};
use pagebridge_mcp::handle_line;
use serde_json::{json, Value};

async fn start_bridge() -> Arc<Pagebridge> {
    let storage = Arc::new(MemoryAdapter::new());
    let echo = Arc::new(EchoLlmProvider::new());
    for _ in 0..30 {
        echo.push_json(json!({
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
    Arc::new(bridge)
}

#[tokio::test]
async fn initialize_advertises_pagebridge() {
    let bridge = start_bridge().await;
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let resp = handle_line(&bridge, &req.to_string()).await.unwrap();
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["serverInfo"]["name"], "pagebridge");
    assert!(v["result"]["protocolVersion"].is_string());
}

#[tokio::test]
async fn tools_list_includes_known_set() {
    let bridge = start_bridge().await;
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let resp = handle_line(&bridge, &req.to_string()).await.unwrap();
    let v: Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "pagebridge.ask",
        "pagebridge.search",
        "pagebridge.list_documents",
        "pagebridge.read_node",
        "pagebridge.children",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
}

#[tokio::test]
async fn list_documents_tool_returns_real_data() {
    let bridge = start_bridge().await;
    let req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": "pagebridge.list_documents", "arguments": {} }
    });
    let resp = handle_line(&bridge, &req.to_string()).await.unwrap();
    let v: Value = serde_json::from_str(&resp).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"title\""));
}

#[tokio::test]
async fn unknown_method_returns_minus_32601() {
    let bridge = start_bridge().await;
    let req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/banana"
    });
    let resp = handle_line(&bridge, &req.to_string()).await.unwrap();
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32601);
}
