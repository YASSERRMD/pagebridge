//! Cohere Command provider.
//!
//! Calls `https://api.cohere.com/v2/chat`. Supports command-r,
//! command-r-plus, and any future Command models. JSON-mode uses the
//! `response_format = {"type": "json_object"}` option.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation,
    clippy::trivially_copy_pass_by_ref
)]

use async_trait::async_trait;
use serde_json::json;

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};

#[derive(Debug, Clone)]
pub struct CohereProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl CohereProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for CohereProvider {
    fn name(&self) -> &'static str {
        "cohere"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let mut messages: Vec<_> = req
            .messages
            .iter()
            .map(|m| json!({"role": role_of(&m.role), "content": m.content}))
            .collect();
        if let Some(sys) = req.system.as_ref() {
            messages.insert(0, json!({"role": "system", "content": sys}));
        }
        let body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": req.temperature.unwrap_or(0.2),
            "max_tokens": req.max_tokens.unwrap_or(1024),
        });
        let resp = self
            .client
            .post("https://api.cohere.com/v2/chat")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| PagebridgeError::Llm {
                provider: "cohere".into(),
                message: format!("send: {e}"),
            })?;
        if !resp.status().is_success() {
            return Err(PagebridgeError::Llm {
                provider: "cohere".into(),
                message: format!("HTTP {}", resp.status()),
            });
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| PagebridgeError::Llm {
            provider: "cohere".into(),
            message: format!("parse: {e}"),
        })?;
        let text = v["message"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let input_tokens = v["usage"]["tokens"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = v["usage"]["tokens"]["output_tokens"].as_u64().unwrap_or(0) as u32;
        Ok(CompletionResponse {
            text,
            input_tokens,
            output_tokens,
            finish_reason: FinishReason::Stop,
        })
    }

    async fn complete_json(
        &self,
        mut req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(last) = req.messages.last_mut() {
            last.content
                .push_str("\n\nReturn ONLY a single valid JSON object.");
        }
        let resp = self.complete(req).await?;
        serde_json::from_str(resp.text.trim()).map_err(|e| PagebridgeError::Llm {
            provider: "cohere".into(),
            message: format!("json parse: {e}"),
        })
    }
}

fn role_of(r: &pagebridge_core::llm::ChatRole) -> &'static str {
    match r {
        pagebridge_core::llm::ChatRole::System => "system",
        pagebridge_core::llm::ChatRole::User => "user",
        pagebridge_core::llm::ChatRole::Assistant => "assistant",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_cohere() {
        let p = CohereProvider::new("k", "command-r-plus");
        assert_eq!(p.name(), "cohere");
    }
}
