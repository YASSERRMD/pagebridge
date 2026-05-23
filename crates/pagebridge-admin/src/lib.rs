//! Built-in admin HTTP server for pagebridge.
//!
//! Exposes a small JSON API plus an embedded single-page admin UI (Alpine +
//! Tailwind, no build step). Intended for single-user local-host deployments
//! by default; remote bindings require `--insecure-allow-remote` from the CLI.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::StreamExt;
use pagebridge_core::error::Result;
use pagebridge_core::{AnswerChunk, DocId, NodeId, Pagebridge};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

const INDEX_HTML: &str = include_str!("../assets/index.html");

/// Run the admin HTTP server on `addr`, backed by `bridge`.
///
/// `addr` must be `127.0.0.1:*` unless the caller explicitly opts into a
/// remote bind. Use [`serve_with_options`] to grant remote access.
pub async fn serve(bridge: Pagebridge, addr: SocketAddr) -> Result<()> {
    serve_with_options(bridge, addr, AdminOptions::default()).await
}

/// Knobs for the admin server.
#[derive(Debug, Clone, Default)]
pub struct AdminOptions {
    /// Allow binding to non-loopback addresses. Off by default.
    pub allow_remote: bool,
}

/// Run the admin server with explicit options.
pub async fn serve_with_options(
    bridge: Pagebridge,
    addr: SocketAddr,
    opts: AdminOptions,
) -> Result<()> {
    if !opts.allow_remote && !addr.ip().is_loopback() {
        return Err(pagebridge_core::error::PagebridgeError::InvalidArgument(
            format!("refusing to bind {addr}: set allow_remote=true to opt in"),
        ));
    }
    let app = router(bridge);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| io_err("bind", e))?;
    tracing::info!("pagebridge admin listening on http://{}", addr);
    axum::serve(listener, app)
        .await
        .map_err(|e| io_err("serve", e))?;
    Ok(())
}

fn io_err(ctx: &str, e: impl std::fmt::Display) -> pagebridge_core::error::PagebridgeError {
    pagebridge_core::error::PagebridgeError::Internal(format!("admin {ctx}: {e}"))
}

#[derive(Clone)]
struct AppState {
    bridge: Arc<Pagebridge>,
}

/// Build the Axum router. Exposed for in-process testing.
#[must_use]
pub fn router(bridge: Pagebridge) -> Router {
    let state = AppState {
        bridge: Arc::new(bridge),
    };
    Router::new()
        .route("/", get(index_html))
        .route("/api/health", get(health))
        .route("/api/stats", get(stats))
        .route("/api/documents", get(list_docs))
        .route("/api/documents/:id", delete(delete_doc))
        .route("/api/nodes/:id", get(get_node))
        .route("/api/nodes/:id/children", get(get_children))
        .route("/api/ask", post(ask))
        .route("/api/ask/stream", post(ask_stream))
        .with_state(state)
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
}

async fn index_html() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn stats(State(s): State<AppState>) -> impl IntoResponse {
    match s.bridge.stats().await {
        Ok(v) => Json(serde_json::to_value(v).unwrap_or_default()).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_docs(State(s): State<AppState>) -> impl IntoResponse {
    match s.bridge.list_documents().await {
        Ok(v) => Json(v).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn delete_doc(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let Ok(did) = DocId::new(&id) else {
        return api_error(StatusCode::BAD_REQUEST, "invalid doc id");
    };
    match s.bridge.remove_document(&did).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_node(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let Ok(nid) = NodeId::new(&id) else {
        return api_error(StatusCode::BAD_REQUEST, "invalid node id");
    };
    match s.bridge.storage().get_node(&nid).await {
        Ok(Some(n)) => Json(n).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "node not found"),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn get_children(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let Ok(nid) = NodeId::new(&id) else {
        return api_error(StatusCode::BAD_REQUEST, "invalid node id");
    };
    match s.bridge.storage().children_summaries(&nid).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct AskBody {
    question: String,
    #[serde(default)]
    doc_id: Option<String>,
}

#[derive(Serialize)]
struct AskResp {
    text: String,
    citations: Vec<pagebridge_core::Citation>,
    trace: pagebridge_core::QueryTrace,
}

async fn ask(State(s): State<AppState>, Json(body): Json<AskBody>) -> impl IntoResponse {
    let res = if let Some(d) = body.doc_id {
        let Ok(did) = DocId::new(&d) else {
            return api_error(StatusCode::BAD_REQUEST, "invalid doc id");
        };
        s.bridge.ask_in_doc(&did, &body.question).await
    } else {
        s.bridge.ask(&body.question).await
    };
    match res {
        Ok(a) => Json(AskResp {
            text: a.text,
            citations: a.citations,
            trace: a.trace,
        })
        .into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn ask_stream(State(s): State<AppState>, Json(body): Json<AskBody>) -> Response {
    let stream_result = if let Some(d) = body.doc_id {
        match DocId::new(&d) {
            Ok(did) => s.bridge.ask_stream_in_doc(&did, &body.question).await,
            Err(_) => return api_error(StatusCode::BAD_REQUEST, "invalid doc id"),
        }
    } else {
        s.bridge.ask_stream(&body.question).await
    };
    let mut events = match stream_result {
        Ok(s) => s,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let body_stream = async_stream::stream! {
        while let Some(item) = events.next().await {
            match item {
                Ok(chunk) => {
                    let line = serialize_chunk(&chunk);
                    yield Ok::<_, std::io::Error>(line);
                }
                Err(e) => {
                    let payload = serde_json::json!({"kind": "error", "message": e.to_string()});
                    yield Ok(payload.to_string() + "\n");
                    break;
                }
            }
        }
    };
    let body = Body::from_stream(body_stream.map(|r: std::io::Result<String>| {
        r.map(axum::body::Bytes::from)
    }));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn serialize_chunk(chunk: &AnswerChunk) -> String {
    serde_json::to_string(chunk)
        .map(|s| s + "\n")
        .unwrap_or_else(|_| String::from("{\"kind\":\"error\"}\n"))
}

fn api_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
