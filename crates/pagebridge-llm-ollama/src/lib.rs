//! Ollama LLM provider for pagebridge.
//!
//! Talks to a local (or remote) Ollama server via HTTP. Uses the `/api/chat`
//! endpoint. Streaming is disabled in v0.1.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::unused_self,
    clippy::option_if_let_else,
    clippy::single_match_else
)]

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, CompletionStream, FinishReason, LlmConfig, LlmProvider,
    RateLimits, StreamChunk,
};
use serde::{Deserialize, Serialize};

/// Default Ollama base URL.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";
/// Default Ollama model.
pub const DEFAULT_MODEL: &str = "qwen2.5:7b";

/// HTTP-backed Ollama LLM provider.
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    base_url: String,
    model: String,
    config: LlmConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Construct with explicit URL and model, using default config.
    pub fn new(url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_config(url, model, LlmConfig::default())
    }

    /// Construct with explicit URL, model, and configuration.
    pub fn with_config(
        url: impl Into<String>,
        model: impl Into<String>,
        config: LlmConfig,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: url.into().trim_end_matches('/').to_owned(),
            model: model.into(),
            config,
            client,
        }
    }

    /// Local default: `http://localhost:11434` with the bundled default model.
    #[must_use]
    pub fn local_default() -> Self {
        Self::new(DEFAULT_BASE_URL, DEFAULT_MODEL)
    }

    fn err<E: std::fmt::Display>(&self, ctx: &str, e: E) -> PagebridgeError {
        PagebridgeError::Llm {
            provider: "ollama".into(),
            message: format!("{ctx}: {e}"),
        }
    }
}

#[derive(Serialize)]
struct OllamaChatReq<'a> {
    model: &'a str,
    messages: Vec<OllamaMsg<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize, Default)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
}

#[derive(Deserialize)]
struct OllamaChatResp {
    message: OllamaMsgResp,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct OllamaMsgResp {
    #[serde(default)]
    content: String,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let resp = do_chat(self, &req, None).await?;
        Ok(resp)
    }

    async fn complete_json(
        &self,
        req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut req = req;
        // Reinforce JSON output via a system message reminder if not present.
        let reminder = "Respond with a single valid JSON object and no surrounding prose.";
        let already = req
            .system
            .as_ref()
            .is_some_and(|s| s.contains("JSON") || s.contains("json"));
        if !already {
            req.system = Some(match req.system.take() {
                Some(s) => format!("{s}\n{reminder}"),
                None => reminder.to_owned(),
            });
        }
        let resp = do_chat(self, &req, Some("json")).await?;
        let v: serde_json::Value = match serde_json::from_str(resp.text.trim()) {
            Ok(v) => v,
            Err(_) => {
                // One retry with a tighter reminder appended to the user message.
                let mut retry = req.clone();
                if let Some(last) = retry.messages.last_mut() {
                    last.content
                        .push_str("\n\nReturn ONLY valid JSON. No prose.");
                }
                let resp2 = do_chat(self, &retry, Some("json")).await?;
                serde_json::from_str(resp2.text.trim()).map_err(|e| PagebridgeError::Llm {
                    provider: "ollama".into(),
                    message: format!("json parse: {e}"),
                })?
            }
        };
        Ok(v)
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<CompletionStream> {
        do_chat_stream(self, req).await
    }

    fn supports_streaming(&self) -> bool {
        true
    }
    fn supports_grammar(&self) -> bool {
        false
    }

    fn rate_limits(&self) -> RateLimits {
        RateLimits::local()
    }
}

async fn do_chat(
    me: &OllamaProvider,
    req: &CompletionRequest,
    format: Option<&str>,
) -> Result<CompletionResponse> {
    let mut messages: Vec<OllamaMsg> = Vec::with_capacity(req.messages.len() + 1);
    if let Some(sys) = &req.system {
        messages.push(OllamaMsg {
            role: "system",
            content: sys,
        });
    }
    for m in &req.messages {
        let role = match m.role {
            pagebridge_core::llm::ChatRole::System => "system",
            pagebridge_core::llm::ChatRole::User => "user",
            pagebridge_core::llm::ChatRole::Assistant => "assistant",
        };
        messages.push(OllamaMsg {
            role,
            content: &m.content,
        });
    }
    let body = OllamaChatReq {
        model: &me.model,
        messages,
        stream: false,
        format: format.map(str::to_owned),
        options: OllamaOptions {
            temperature: req.temperature.or(Some(me.config.default_temperature)),
            num_predict: req.max_tokens.or(Some(me.config.default_max_tokens)),
            stop: req.stop.clone(),
        },
    };
    let url = format!("{}/api/chat", me.base_url);
    let mut attempt = 0u32;
    loop {
        let res = me.client.post(&url).json(&body).send().await;
        match res {
            Ok(r) if r.status().is_success() => {
                let parsed: OllamaChatResp = r.json().await.map_err(|e| me.err("decode", e))?;
                return Ok(CompletionResponse {
                    text: parsed.message.content,
                    input_tokens: parsed.prompt_eval_count,
                    output_tokens: parsed.eval_count,
                    finish_reason: if parsed.done {
                        FinishReason::Stop
                    } else {
                        FinishReason::Length
                    },
                });
            }
            Ok(r) if r.status().is_server_error() && attempt < me.config.max_retries => {
                let status = r.status();
                tracing::warn!(
                    "ollama transient {} on attempt {}, retrying",
                    status,
                    attempt + 1
                );
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                return Err(me.err(&format!("status {status}"), body));
            }
            Err(e) if attempt < me.config.max_retries && (e.is_timeout() || e.is_connect()) => {
                tracing::warn!("ollama transient error {e} on attempt {}", attempt + 1);
            }
            Err(e) => {
                return Err(me.err("send", e));
            }
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(
            me.config.retry_backoff_ms * u64::from(attempt),
        ))
        .await;
    }
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    message: Option<OllamaStreamMsg>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaStreamMsg {
    #[serde(default)]
    content: String,
}

async fn do_chat_stream(me: &OllamaProvider, req: CompletionRequest) -> Result<CompletionStream> {
    let mut messages: Vec<OllamaMsg> = Vec::with_capacity(req.messages.len() + 1);
    if let Some(sys) = &req.system {
        messages.push(OllamaMsg {
            role: "system",
            content: sys,
        });
    }
    for m in &req.messages {
        let role = match m.role {
            pagebridge_core::llm::ChatRole::System => "system",
            pagebridge_core::llm::ChatRole::User => "user",
            pagebridge_core::llm::ChatRole::Assistant => "assistant",
        };
        messages.push(OllamaMsg {
            role,
            content: &m.content,
        });
    }
    let body = OllamaChatReq {
        model: &me.model,
        messages,
        stream: true,
        format: None,
        options: OllamaOptions {
            temperature: req.temperature.or(Some(me.config.default_temperature)),
            num_predict: req.max_tokens.or(Some(me.config.default_max_tokens)),
            stop: req.stop.clone(),
        },
    };
    let url = format!("{}/api/chat", me.base_url);
    let resp = me
        .client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| me.err("stream send", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(me.err(&format!("stream status {status}"), body));
    }
    let bytes = resp.bytes_stream();
    let provider_name = "ollama";
    let mapped = async_stream::try_stream! {
        let mut buf: Vec<u8> = Vec::new();
        let mut bs = bytes;
        let mut prompt_tokens = 0u32;
        let mut eval_tokens = 0u32;
        let mut done_seen = false;
        while let Some(item) = bs.next().await {
            let chunk = item.map_err(|e| PagebridgeError::Llm {
                provider: provider_name.into(),
                message: format!("read chunk: {e}"),
            })?;
            buf.extend_from_slice(&chunk);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line = buf.drain(..=nl).collect::<Vec<u8>>();
                let trimmed = std::str::from_utf8(&line).unwrap_or("").trim();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed: OllamaStreamChunk = match serde_json::from_str(trimmed) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if let Some(msg) = parsed.message {
                    if !msg.content.is_empty() {
                        yield StreamChunk::Token(msg.content);
                    }
                }
                if parsed.done {
                    prompt_tokens = parsed.prompt_eval_count.unwrap_or(prompt_tokens);
                    eval_tokens = parsed.eval_count.unwrap_or(eval_tokens);
                    done_seen = true;
                }
            }
        }
        if !done_seen {
            // Server closed without a done frame; still emit the terminal chunk.
        }
        yield StreamChunk::Finished {
            input_tokens: prompt_tokens,
            output_tokens: eval_tokens,
            finish_reason: FinishReason::Stop,
        };
    };
    Ok(Box::pin(mapped))
}
