//! wiremock-driven tests for the OpenAI-compatible provider.

#![allow(clippy::redundant_clone)]

use pagebridge_core::llm::{CompletionRequest, LlmConfig, LlmProvider};
use pagebridge_llm_openai::OpenAiCompatibleProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ok_body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 9, "completion_tokens": 11}
    })
}

#[tokio::test]
async fn complete_basic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("hello world")))
        .mount(&server)
        .await;

    let p = OpenAiCompatibleProvider::custom(server.uri(), Some("sk-test".into()), "gpt-4o-mini");
    let resp = p.complete(CompletionRequest::user("hi")).await.unwrap();
    assert!(resp.text.contains("hello"));
    assert_eq!(resp.input_tokens, 9);
    assert_eq!(resp.output_tokens, 11);
}

#[tokio::test]
async fn json_mode_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ok_body("{\"action\":\"descend\",\"node_id\":\"x\"}")),
        )
        .mount(&server)
        .await;

    let p = OpenAiCompatibleProvider::vllm(server.uri(), "qwen2.5");
    let v = p
        .complete_json(CompletionRequest::user("decide"), &serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(v["action"], "descend");
}

#[tokio::test]
async fn rate_limit_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("ok")))
        .mount(&server)
        .await;

    let p = OpenAiCompatibleProvider::with_config(
        server.uri(),
        None,
        "gpt-4o-mini",
        LlmConfig {
            max_retries: 3,
            retry_backoff_ms: 1,
            ..LlmConfig::default()
        },
    );
    let resp = p.complete(CompletionRequest::user("ping")).await.unwrap();
    assert_eq!(resp.text, "ok");
}
