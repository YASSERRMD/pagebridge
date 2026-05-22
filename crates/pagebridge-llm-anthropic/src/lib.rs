//! Anthropic LLM provider for pagebridge.
//!
//! Uses the Messages API (`/v1/messages`). JSON mode is implemented via the
//! tool-use forcing trick: declare a single tool whose input schema matches
//! the requested JSON schema and force the model to call it.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::unused_self,
    clippy::option_if_let_else,
    clippy::single_match_else,
    clippy::or_fun_call,
    clippy::match_same_arms
)]

use std::time::Duration;

use async_trait::async_trait;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmConfig, LlmProvider,
};
use serde::{Deserialize, Serialize};

/// Default Anthropic base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Default model used when the caller does not specify one.
pub const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic LLM provider.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    model: String,
    config: LlmConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Construct using the official Anthropic endpoint.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_url(DEFAULT_BASE_URL, api_key, model, LlmConfig::default())
    }

    /// Construct with an explicit base URL (useful for proxies and tests).
    pub fn with_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        config: LlmConfig,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
            config,
            client,
        }
    }

    fn err<E: std::fmt::Display>(&self, ctx: &str, e: E) -> PagebridgeError {
        PagebridgeError::Llm {
            provider: "anthropic".into(),
            message: format!("{ctx}: {e}"),
        }
    }
}

#[derive(Serialize)]
struct MsgReq<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<MsgItem<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop_sequences: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
}

#[derive(Serialize)]
struct MsgItem<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct Tool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Serialize)]
struct ToolChoice {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
}

#[derive(Deserialize)]
struct MsgResp {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: AnthroUsage,
}

#[derive(Deserialize, Default)]
struct AnthroUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        #[serde(default)]
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        do_messages(self, &req, None).await
    }

    async fn complete_json(
        &self,
        req: CompletionRequest,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Use the tool-call forcing trick: declare one tool with input_schema = schema.
        let tool_name = "respond";
        let resp = do_messages(self, &req, Some((tool_name.to_owned(), schema.clone()))).await?;
        match serde_json::from_str::<serde_json::Value>(&resp.text) {
            Ok(v) => Ok(v),
            Err(e) => Err(PagebridgeError::Llm {
                provider: "anthropic".into(),
                message: format!("tool-use input was not JSON: {e}"),
            }),
        }
    }

    fn supports_grammar(&self) -> bool {
        true
    }
}

async fn do_messages(
    me: &AnthropicProvider,
    req: &CompletionRequest,
    json_tool: Option<(String, serde_json::Value)>,
) -> Result<CompletionResponse> {
    let messages: Vec<MsgItem> = req
        .messages
        .iter()
        .map(|m| MsgItem {
            role: match m.role {
                pagebridge_core::llm::ChatRole::Assistant => "assistant",
                _ => "user",
            },
            content: &m.content,
        })
        .collect();
    let tools = json_tool
        .as_ref()
        .map(|(name, schema)| {
            vec![Tool {
                name: name.clone(),
                description: "Reply by calling this tool with the requested JSON.".into(),
                input_schema: schema.clone(),
            }]
        })
        .unwrap_or_default();
    let tool_choice = json_tool.as_ref().map(|(name, _)| ToolChoice {
        kind: "tool",
        name: name.clone(),
    });
    let body = MsgReq {
        model: &me.model,
        max_tokens: req.max_tokens.unwrap_or(me.config.default_max_tokens),
        system: req.system.as_deref(),
        messages,
        temperature: req.temperature.or(Some(me.config.default_temperature)),
        stop_sequences: req.stop.clone(),
        tools,
        tool_choice,
    };

    let url = format!("{}/v1/messages", me.base_url);
    let mut attempt = 0u32;
    loop {
        let rb = me
            .client
            .post(&url)
            .header("x-api-key", &me.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body);
        match rb.send().await {
            Ok(r) if r.status().is_success() => {
                let parsed: MsgResp = r.json().await.map_err(|e| me.err("decode", e))?;
                let finish = match parsed.stop_reason.as_deref() {
                    Some("end_turn") | None => FinishReason::Stop,
                    Some("max_tokens") => FinishReason::Length,
                    Some("tool_use") => FinishReason::ToolCall,
                    _ => FinishReason::Stop,
                };
                let mut text = String::new();
                for block in parsed.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(&t),
                        ContentBlock::ToolUse { input } => {
                            text.push_str(&serde_json::to_string(&input).unwrap_or_default());
                        }
                        ContentBlock::Other => {}
                    }
                }
                return Ok(CompletionResponse {
                    text,
                    input_tokens: parsed.usage.input_tokens,
                    output_tokens: parsed.usage.output_tokens,
                    finish_reason: finish,
                });
            }
            Ok(r) if r.status().as_u16() == 429 && attempt < me.config.max_retries => {
                let wait = r
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or((me.config.retry_backoff_ms * u64::from(attempt + 1)) / 1000)
                    .max(1);
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Ok(r) if r.status().is_server_error() && attempt < me.config.max_retries => {
                let s = r.status();
                tracing::warn!("anthropic transient {s} attempt {}", attempt + 1);
            }
            Ok(r) => {
                let s = r.status();
                let b = r.text().await.unwrap_or_default();
                return Err(me.err(&format!("status {s}"), b));
            }
            Err(e) if attempt < me.config.max_retries && (e.is_timeout() || e.is_connect()) => {
                tracing::warn!("anthropic transient error {e} attempt {}", attempt + 1);
            }
            Err(e) => return Err(me.err("send", e)),
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(
            me.config.retry_backoff_ms * u64::from(attempt),
        ))
        .await;
    }
}
