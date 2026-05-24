//! OpenAI-compatible LLM provider.
//!
//! Targets the OpenAI Chat Completions API (`/v1/chat/completions`) and the
//! many open-source servers that implement the same shape (vLLM, LM Studio,
//! LocalAI, llama-server, etc.).

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
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::cognitive_complexity,
    clippy::missing_const_for_fn
)]

use std::time::Duration;

use async_trait::async_trait;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmConfig, LlmProvider, RateLimits,
};
use serde::{Deserialize, Serialize};

/// OpenAI-compatible LLM provider with multiple constructors covering common
/// open-source servers.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: Option<String>,
    model: String,
    config: LlmConfig,
    client: reqwest::Client,
    rate_limits: RateLimits,
}

impl OpenAiCompatibleProvider {
    /// Connect to OpenAI proper with the given API key. Pre-loaded with
    /// [`RateLimits::openai_tier_1`].
    pub fn openai(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom("https://api.openai.com", Some(api_key.into()), model)
            .with_rate_limits(RateLimits::openai_tier_1())
    }

    /// Connect to a custom OpenAI-compatible server.
    pub fn custom(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::with_config(base_url, api_key, model, LlmConfig::default())
    }

    /// Connect to a vLLM server (no API key by default).
    pub fn vllm(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom(base_url, None, model)
    }

    /// Connect to LM Studio at its default `http://localhost:1234/v1`.
    pub fn lm_studio(model: impl Into<String>) -> Self {
        Self::custom("http://localhost:1234", None, model)
    }

    /// Connect to Groq (OpenAI-compatible) with the given API key. Pre-loaded
    /// with [`RateLimits::groq_free`]; call
    /// [`OpenAiCompatibleProvider::with_rate_limits`] to switch to
    /// [`RateLimits::groq_paid`] when on a paid plan.
    pub fn groq(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom("https://api.groq.com/openai", Some(api_key.into()), model)
            .with_rate_limits(RateLimits::groq_free())
    }

    /// Connect to Cerebras (OpenAI-compatible) with the given API key.
    pub fn cerebras(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom("https://api.cerebras.ai", Some(api_key.into()), model)
    }

    /// Connect to Fireworks AI (OpenAI-compatible) with the given API key.
    pub fn fireworks(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom(
            "https://api.fireworks.ai/inference",
            Some(api_key.into()),
            model,
        )
    }

    /// Connect to Together AI (OpenAI-compatible) with the given API key.
    pub fn together(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom("https://api.together.xyz", Some(api_key.into()), model)
    }

    /// Connect to Mistral (OpenAI-compatible) with the given API key.
    pub fn mistral(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom("https://api.mistral.ai", Some(api_key.into()), model)
    }

    /// Connect to a Hugging Face TGI server.
    pub fn hf_tgi(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom(base_url, None, model)
    }

    /// Connect to Azure OpenAI. Azure URLs typically look like
    /// `https://<resource>.openai.azure.com/openai/deployments/<deployment>`.
    pub fn azure_openai(
        resource_url: impl Into<String>,
        api_key: impl Into<String>,
        deployment: impl Into<String>,
    ) -> Self {
        Self::custom(resource_url, Some(api_key.into()), deployment)
    }

    /// Connect to Replicate (OpenAI-compatible adapter endpoint).
    pub fn replicate(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom(
            "https://openai-proxy.replicate.com",
            Some(api_key.into()),
            model,
        )
    }

    /// Connect to a Modal deployment exposing an OpenAI-compatible /v1 endpoint.
    pub fn modal(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::custom(base_url, None, model)
    }

    /// Construct with full configuration.
    pub fn with_config(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        config: LlmConfig,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            model: model.into(),
            config,
            client,
            rate_limits: RateLimits::unlimited(),
        }
    }

    /// Override the declared rate limits. Use one of the
    /// [`RateLimits`] presets (`groq_free`, `groq_paid`, `openai_tier_1`, etc).
    #[must_use]
    pub fn with_rate_limits(mut self, limits: RateLimits) -> Self {
        self.rate_limits = limits;
        self
    }

    fn err<E: std::fmt::Display>(&self, ctx: &str, e: E) -> PagebridgeError {
        PagebridgeError::Llm {
            provider: "openai".into(),
            message: format!("{ctx}: {e}"),
        }
    }
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<ChatMsg<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct ChatMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatResp {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    message: ChoiceMsg,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChoiceMsg {
    #[serde(default)]
    content: String,
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        "openai"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        do_chat(self, &req, false).await
    }

    fn rate_limits(&self) -> RateLimits {
        self.rate_limits
    }

    async fn complete_json(
        &self,
        req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut r = req;
        let reminder = "Respond with a single valid JSON object and no surrounding prose.";
        let already = r
            .system
            .as_ref()
            .is_some_and(|s| s.contains("JSON") || s.contains("json"));
        if !already {
            r.system = Some(match r.system.take() {
                Some(s) => format!("{s}\n{reminder}"),
                None => reminder.to_owned(),
            });
        }
        let resp = do_chat(self, &r, true).await?;
        match serde_json::from_str::<serde_json::Value>(resp.text.trim()) {
            Ok(v) => Ok(v),
            Err(_) => {
                let mut retry = r.clone();
                if let Some(last) = retry.messages.last_mut() {
                    last.content.push_str("\n\nReturn ONLY valid JSON.");
                }
                let r2 = do_chat(self, &retry, true).await?;
                serde_json::from_str::<serde_json::Value>(r2.text.trim()).map_err(|e| {
                    PagebridgeError::Llm {
                        provider: "openai".into(),
                        message: format!("json parse: {e}"),
                    }
                })
            }
        }
    }
}

async fn do_chat(
    me: &OpenAiCompatibleProvider,
    req: &CompletionRequest,
    json_mode: bool,
) -> Result<CompletionResponse> {
    let mut messages: Vec<ChatMsg> = Vec::with_capacity(req.messages.len() + 1);
    if let Some(sys) = &req.system {
        messages.push(ChatMsg {
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
        messages.push(ChatMsg {
            role,
            content: &m.content,
        });
    }
    let body = ChatReq {
        model: &me.model,
        messages,
        temperature: req.temperature.or(Some(me.config.default_temperature)),
        max_tokens: req.max_tokens.or(Some(me.config.default_max_tokens)),
        stop: req.stop.clone(),
        stream: false,
        response_format: if json_mode {
            Some(ResponseFormat {
                kind: "json_object",
            })
        } else {
            None
        },
    };
    let url = format!("{}/v1/chat/completions", me.base_url);
    let mut attempt = 0u32;
    loop {
        let mut rb = me.client.post(&url).json(&body);
        if let Some(k) = &me.api_key {
            rb = rb.bearer_auth(k);
        }
        match rb.send().await {
            Ok(r) if r.status().is_success() => {
                let parsed: ChatResp = r.json().await.map_err(|e| me.err("decode", e))?;
                let choice = parsed
                    .choices
                    .into_iter()
                    .next()
                    .ok_or_else(|| me.err("choices", "empty"))?;
                let finish = match choice.finish_reason.as_deref() {
                    Some("stop") => FinishReason::Stop,
                    Some("length") => FinishReason::Length,
                    Some("tool_calls") => FinishReason::ToolCall,
                    _ => FinishReason::Stop,
                };
                return Ok(CompletionResponse {
                    text: choice.message.content,
                    input_tokens: parsed.usage.prompt_tokens,
                    output_tokens: parsed.usage.completion_tokens,
                    finish_reason: finish,
                });
            }
            Ok(r) if r.status().as_u16() == 429 && attempt < me.config.max_retries => {
                let wait = r
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or((me.config.retry_backoff_ms * u64::from(attempt + 1)) / 1000)
                    .max(1);
                tracing::warn!("openai 429, sleeping {wait}s");
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Ok(r) if r.status().is_server_error() && attempt < me.config.max_retries => {
                let status = r.status();
                tracing::warn!("openai transient {status} attempt {}", attempt + 1);
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                return Err(me.err(&format!("status {status}"), body));
            }
            Err(e) if attempt < me.config.max_retries && (e.is_timeout() || e.is_connect()) => {
                tracing::warn!("openai transient error {e} attempt {}", attempt + 1);
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
