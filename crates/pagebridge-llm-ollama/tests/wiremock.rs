//! wiremock-driven tests for the Ollama provider.

#![allow(clippy::redundant_clone)]

use pagebridge_core::llm::{ChatMessage, CompletionRequest, LlmConfig, LlmProvider};
use pagebridge_llm_ollama::OllamaProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn complete_basic() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "message": {"role": "assistant", "content": "hello world"},
        "prompt_eval_count": 7,
        "eval_count": 12,
        "done": true,
    });
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let p = OllamaProvider::new(server.uri(), "qwen2.5:7b");
    let resp = p
        .complete(CompletionRequest::user("greet me"))
        .await
        .unwrap();
    assert!(resp.text.contains("hello"));
    assert_eq!(resp.input_tokens, 7);
    assert_eq!(resp.output_tokens, 12);
}

#[tokio::test]
async fn complete_json_parses() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "message": {"role": "assistant", "content": "{\"action\":\"descend\",\"node_id\":\"doc:x/sec:1\"}"},
        "prompt_eval_count": 10,
        "eval_count": 20,
        "done": true,
    });
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let p = OllamaProvider::new(server.uri(), "qwen2.5:7b");
    let v = p
        .complete_json(
            CompletionRequest {
                system: Some("You are JSON only.".into()),
                messages: vec![ChatMessage::user("decide")],
                ..Default::default()
            },
            &serde_json::json!({}),
        )
        .await
        .unwrap();
    assert_eq!(v["action"], "descend");
}

#[tokio::test]
async fn retries_on_5xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    let body = serde_json::json!({
        "message": {"role": "assistant", "content": "recovered"},
        "prompt_eval_count": 1,
        "eval_count": 1,
        "done": true,
    });
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let p = OllamaProvider::with_config(
        server.uri(),
        "qwen2.5:7b",
        LlmConfig {
            max_retries: 5,
            retry_backoff_ms: 1,
            ..LlmConfig::default()
        },
    );
    let resp = p.complete(CompletionRequest::user("ping")).await.unwrap();
    assert_eq!(resp.text, "recovered");
}
