//! Model Context Protocol server for pagebridge.
//!
//! Implements JSON-RPC 2.0 over newline-delimited stdio, the transport every
//! current MCP host (Claude Code, Claude Desktop, Cursor) speaks. The exposed
//! tool surface mirrors the public `Pagebridge` facade: `ask`, `search`,
//! `list_documents`, `read_node`, `children`. Resources expose each document
//! at `pagebridge://docs/<doc_id>`.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::too_many_lines
)]

use std::sync::Arc;

use pagebridge_core::Pagebridge;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub mod handler;
pub mod protocol;

/// Run the MCP server reading JSON-RPC requests from stdin and writing
/// responses to stdout. Blocks until stdin is closed.
pub async fn serve_stdio(bridge: Pagebridge) -> std::io::Result<()> {
    let bridge = Arc::new(bridge);
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: protocol::Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = protocol::Response::err(
                    Value::Null,
                    -32700,
                    format!("parse error: {e}"),
                );
                write_response(&mut stdout, &resp).await?;
                continue;
            }
        };
        let id = req.id.clone().unwrap_or(Value::Null);
        let resp = handler::dispatch(&bridge, id, &req.method, req.params).await;
        if req.id.is_some() {
            write_response(&mut stdout, &resp).await?;
        }
    }
    Ok(())
}

async fn write_response(
    stdout: &mut tokio::io::Stdout,
    resp: &protocol::Response,
) -> std::io::Result<()> {
    let mut body = serde_json::to_vec(resp).unwrap_or_default();
    body.push(b'\n');
    stdout.write_all(&body).await?;
    stdout.flush().await?;
    Ok(())
}

/// Process a single MCP JSON-RPC request line and return the response line.
/// Exposed for tests so callers can avoid plumbing real stdin/stdout.
pub async fn handle_line(bridge: &Arc<Pagebridge>, line: &str) -> Option<String> {
    let req: protocol::Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp = protocol::Response::err(
                Value::Null,
                -32700,
                format!("parse error: {e}"),
            );
            return Some(serde_json::to_string(&resp).unwrap_or_default());
        }
    };
    let id = req.id.clone().unwrap_or(Value::Null);
    let resp = handler::dispatch(bridge, id, &req.method, req.params).await;
    req.id
        .as_ref()
        .map(|_| serde_json::to_string(&resp).unwrap_or_default())
}
