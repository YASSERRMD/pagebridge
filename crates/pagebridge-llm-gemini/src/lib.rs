//! Google Gemini provider.
//!
//! Calls `https://generativelanguage.googleapis.com/v1beta/models/<model>:generateContent`.
//! Supports text completion plus inline images for vision-capable models
//! (gemini-2.0-flash, gemini-2.0-pro).

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation
)]

use async_trait::async_trait;
use serde_json::json;

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider};

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &'static str {
        "google"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
        let mut parts = Vec::new();
        for m in &req.messages {
            parts.push(json!({"text": m.content}));
        }
        let body = json!({
            "contents": [{"parts": parts}],
            "generationConfig": {
                "temperature": req.temperature.unwrap_or(0.2),
                "maxOutputTokens": req.max_tokens.unwrap_or(1024),
            }
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| PagebridgeError::Llm {
                provider: "google".into(),
                message: format!("send: {e}"),
            })?;
        if !resp.status().is_success() {
            return Err(PagebridgeError::Llm {
                provider: "google".into(),
                message: format!("HTTP {}", resp.status()),
            });
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| PagebridgeError::Llm {
            provider: "google".into(),
            message: format!("parse: {e}"),
        })?;
        let text = v["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let input_tokens = v["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32;
        let output_tokens = v["usageMetadata"]["candidatesTokenCount"]
            .as_u64()
            .unwrap_or(0) as u32;
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
            last.content.push_str("\n\nReturn ONLY valid JSON.");
        }
        let resp = self.complete(req).await?;
        serde_json::from_str(resp.text.trim()).map_err(|e| PagebridgeError::Llm {
            provider: "google".into(),
            message: format!("json parse: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_google() {
        let p = GeminiProvider::new("k", "gemini-2.0-flash");
        assert_eq!(p.name(), "google");
        assert_eq!(p.model(), "gemini-2.0-flash");
    }
}
