//! Dispatch for the MCP method set exposed by pagebridge.

use std::sync::Arc;

use pagebridge_core::{DocId, NodeId, Pagebridge};
use serde_json::{json, Value};

use crate::protocol::{Resource, Response, TextContent, ToolResult, PROTOCOL_VERSION};

/// Names + descriptions of every tool the MCP server exposes. Schemas are
/// returned by [`schema_for`] so they don't need to live as `const Value`s.
const TOOL_INDEX: &[(&str, &str)] = &[
    (
        "pagebridge.ask",
        "Ask a natural-language question and get a cited answer.",
    ),
    (
        "pagebridge.search",
        "BM25 search over all leaves. Returns hits without LLM navigation.",
    ),
    (
        "pagebridge.list_documents",
        "List every document the pagebridge instance holds.",
    ),
    ("pagebridge.read_node", "Fetch a single node by id."),
    ("pagebridge.children", "List the children of a node."),
];

fn schema_for(tool_name: &str) -> Value {
    match tool_name {
        "pagebridge.ask" => json!({
            "type": "object",
            "required": ["question"],
            "properties": {
                "question": {"type": "string"},
                "doc_id": {"type": "string"}
            }
        }),
        "pagebridge.search" => json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 10}
            }
        }),
        "pagebridge.list_documents" => json!({"type": "object", "properties": {}}),
        "pagebridge.read_node" | "pagebridge.children" => json!({
            "type": "object",
            "required": ["node_id"],
            "properties": {"node_id": {"type": "string"}}
        }),
        _ => json!({}),
    }
}

/// Dispatcher: takes a request id + method + params and returns a `Response`.
pub async fn dispatch(
    bridge: &Arc<Pagebridge>,
    id: Value,
    method: &str,
    params: Value,
) -> Response {
    match method {
        "initialize" => Response::ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "serverInfo": { "name": "pagebridge", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": {
                    "tools": {},
                    "resources": {}
                }
            }),
        ),
        "tools/list" => {
            let tools: Vec<Value> = TOOL_INDEX
                .iter()
                .map(|(name, description)| {
                    json!({
                        "name": name,
                        "description": description,
                        "inputSchema": schema_for(name),
                    })
                })
                .collect();
            Response::ok(id, json!({ "tools": tools }))
        }
        "tools/call" => match call_tool(bridge, params).await {
            Ok(v) => Response::ok(id, v),
            Err(e) => Response::err(id, -32000, e),
        },
        "resources/list" => match list_resources(bridge).await {
            Ok(v) => Response::ok(id, json!({ "resources": v })),
            Err(e) => Response::err(id, -32000, e),
        },
        "resources/read" => match read_resource(bridge, params).await {
            Ok(v) => Response::ok(id, v),
            Err(e) => Response::err(id, -32000, e),
        },
        "notifications/initialized" | "ping" => Response::ok(id, json!({})),
        other => Response::err(id, -32601, format!("method not found: {other}")),
    }
}

async fn call_tool(bridge: &Arc<Pagebridge>, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing 'name'".to_owned())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match name {
        "pagebridge.ask" => {
            let q = args
                .get("question")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'question'".to_owned())?;
            let answer = if let Some(d) = args.get("doc_id").and_then(Value::as_str) {
                let did = DocId::new(d).map_err(|e| e.to_string())?;
                bridge.ask_in_doc(&did, q).await
            } else {
                bridge.ask(q).await
            }
            .map_err(|e| e.to_string())?;
            Ok(json!(ToolResult {
                content: vec![TextContent::text(format_answer(&answer))],
                is_error: false,
            }))
        }
        "pagebridge.search" => {
            let q = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'query'".to_owned())?;
            let limit = usize::try_from(args.get("limit").and_then(Value::as_i64).unwrap_or(10))
                .unwrap_or(10);
            let hits = bridge
                .bm25_search(q, limit)
                .await
                .map_err(|e| e.to_string())?;
            let body = serde_json::to_string_pretty(&hits).unwrap_or_default();
            Ok(json!(ToolResult {
                content: vec![TextContent::text(body)],
                is_error: false,
            }))
        }
        "pagebridge.list_documents" => {
            let docs = bridge.list_documents().await.map_err(|e| e.to_string())?;
            let body = serde_json::to_string_pretty(&docs).unwrap_or_default();
            Ok(json!(ToolResult {
                content: vec![TextContent::text(body)],
                is_error: false,
            }))
        }
        "pagebridge.read_node" => {
            let nid = args
                .get("node_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'node_id'".to_owned())?;
            let id = NodeId::new(nid).map_err(|e| e.to_string())?;
            let node = bridge
                .storage()
                .get_node(&id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "node not found".to_owned())?;
            let body = serde_json::to_string_pretty(&node).unwrap_or_default();
            Ok(json!(ToolResult {
                content: vec![TextContent::text(body)],
                is_error: false,
            }))
        }
        "pagebridge.children" => {
            let nid = args
                .get("node_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing 'node_id'".to_owned())?;
            let id = NodeId::new(nid).map_err(|e| e.to_string())?;
            let kids = bridge
                .storage()
                .children_summaries(&id)
                .await
                .map_err(|e| e.to_string())?;
            let body = serde_json::to_string_pretty(&kids).unwrap_or_default();
            Ok(json!(ToolResult {
                content: vec![TextContent::text(body)],
                is_error: false,
            }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn format_answer(a: &pagebridge_core::Answer) -> String {
    use std::fmt::Write;
    let mut out = a.text.clone();
    if !a.citations.is_empty() {
        out.push_str("\n\nCitations:\n");
        for (i, c) in a.citations.iter().enumerate() {
            let _ = writeln!(
                out,
                "  [{}] {} / {} ({})",
                i + 1,
                c.doc_title,
                c.section_title,
                c.node_id
            );
        }
    }
    out
}

async fn list_resources(bridge: &Arc<Pagebridge>) -> Result<Vec<Resource>, String> {
    let docs = bridge.list_documents().await.map_err(|e| e.to_string())?;
    Ok(docs
        .into_iter()
        .map(|d| Resource {
            uri: format!("pagebridge://docs/{}", d.doc_id),
            name: d.title,
            description: Some(format!("{} leaves, {} bytes", d.leaf_count, d.byte_count)),
            mime_type: Some("text/plain".into()),
        })
        .collect())
}

async fn read_resource(bridge: &Arc<Pagebridge>, params: Value) -> Result<Value, String> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing 'uri'".to_owned())?;
    let stripped = uri
        .strip_prefix("pagebridge://docs/")
        .ok_or_else(|| format!("unsupported uri: {uri}"))?;
    let did = DocId::new(stripped).map_err(|e| e.to_string())?;
    let docs = bridge.list_documents().await.map_err(|e| e.to_string())?;
    let doc = docs
        .iter()
        .find(|d| d.doc_id == did)
        .ok_or_else(|| "doc not found".to_owned())?;
    let leaves = bridge
        .storage()
        .leaves_under(&doc.root_node_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut full = String::new();
    for leaf_id in &leaves {
        if let Ok(Some(leaf)) = bridge.storage().get_node(leaf_id).await {
            if let Some(span) = leaf.span {
                if let Ok(text) = bridge.storage().read_raw_text(&did, span).await {
                    full.push_str(&text);
                    full.push_str("\n\n");
                    continue;
                }
            }
            full.push_str(&leaf.summary);
            full.push_str("\n\n");
        }
    }
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "text/plain",
            "text": full,
        }]
    }))
}
