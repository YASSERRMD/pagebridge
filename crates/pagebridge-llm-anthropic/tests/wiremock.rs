//! wiremock-driven tests for the Anthropic provider.

#![allow(clippy::redundant_clone, clippy::needless_pass_by_value)]

use pagebridge_core::llm::{CompletionRequest, LlmConfig, LlmProvider};
use pagebridge_llm_anthropic::AnthropicProvider;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn text_body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 8, "output_tokens": 12},
    })
}

fn tool_body(input: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "tool_use", "input": input}],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 8, "output_tokens": 12},
    })
}

#[tokio::test]
async fn complete_basic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(text_body("hello world")))
        .mount(&server)
        .await;

    let p = AnthropicProvider::with_url(
        server.uri(),
        "sk-test",
        "claude-haiku-4-5",
        LlmConfig::default(),
    );
    let resp = p.complete(CompletionRequest::user("hi")).await.unwrap();
    assert!(resp.text.contains("hello"));
    assert_eq!(resp.input_tokens, 8);
}

#[tokio::test]
async fn complete_json_via_tool_use() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tool_body(
            serde_json::json!({"action":"descend","node_id":"x"}),
        )))
        .mount(&server)
        .await;

    let p = AnthropicProvider::with_url(
        server.uri(),
        "sk-test",
        "claude-haiku-4-5",
        LlmConfig::default(),
    );
    let schema = serde_json::json!({"type":"object","properties":{"action":{"type":"string"}}});
    let v = p
        .complete_json(CompletionRequest::user("decide"), &schema)
        .await
        .unwrap();
    assert_eq!(v["action"], "descend");
}
