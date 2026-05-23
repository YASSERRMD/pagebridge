//! The [`LlmProvider`] trait every LLM provider implements.
//!
//! Pagebridge is intentionally LLM-agnostic. Every LLM call goes through this
//! trait. Concrete providers live in their own crates (Ollama, OpenAI, Anthropic).

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::error::Result;

/// Roles for chat messages, mirroring the OpenAI/Anthropic conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// One message in a chat-completion request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    #[must_use]
    pub fn system(s: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: s.into(),
        }
    }
    #[must_use]
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: s.into(),
        }
    }
    #[must_use]
    pub fn assistant(s: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: s.into(),
        }
    }
}

/// Why the provider stopped generating tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCall,
    Error,
}

/// A completion request, provider-neutral.
#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop: Vec<String>,
}

impl CompletionRequest {
    /// Convenience builder: turn a single user prompt into a request.
    #[must_use]
    pub fn user(prompt: impl Into<String>) -> Self {
        Self {
            messages: vec![ChatMessage::user(prompt)],
            ..Self::default()
        }
    }
}

/// A completion response, provider-neutral.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub finish_reason: FinishReason,
}

/// A chunk emitted by a streaming completion.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A piece of generated text. Concatenating every `Token` event yields the
    /// same string a non-streaming `complete()` call would have returned.
    Token(String),
    /// End-of-stream marker with usage counts.
    Finished {
        input_tokens: u32,
        output_tokens: u32,
        finish_reason: FinishReason,
    },
}

/// Boxed asynchronous stream of completion chunks.
pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send + 'static>>;

/// Provider-side knobs (timeouts, retries) shared across all providers.
#[derive(Debug, Clone, Copy)]
pub struct LlmConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    pub default_max_tokens: u32,
    pub default_temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 60,
            max_retries: 2,
            retry_backoff_ms: 500,
            default_max_tokens: 1024,
            default_temperature: 0.2,
        }
    }
}

/// The contract every LLM provider implements.
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    /// Stable provider name, e.g. "ollama", "openai", "anthropic".
    fn name(&self) -> &'static str;

    /// Model identifier, e.g. "qwen2.5:7b", "gpt-4o-mini", "claude-haiku-4-5".
    fn model(&self) -> &str;

    /// Freeform text completion.
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;

    /// JSON-mode completion. The provider should return JSON conforming to
    /// `schema`. Providers that lack native grammar support emulate this by
    /// instructing the model and parsing the response.
    async fn complete_json(
        &self,
        req: CompletionRequest,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Stream tokens from a completion. Default implementation falls back to
    /// `complete()` and emits the whole response as one token followed by a
    /// `Finished` chunk. Providers that natively stream override this method.
    async fn complete_stream(&self, req: CompletionRequest) -> Result<CompletionStream> {
        let resp = self.complete(req).await?;
        let s = futures::stream::iter(vec![
            Ok(StreamChunk::Token(resp.text)),
            Ok(StreamChunk::Finished {
                input_tokens: resp.input_tokens,
                output_tokens: resp.output_tokens,
                finish_reason: resp.finish_reason,
            }),
        ]);
        Ok(Box::pin(s))
    }

    /// Does this provider stream tokens natively? Default false. Streaming via
    /// the default fallback still works regardless.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Does this provider support grammar-constrained JSON? Default false.
    fn supports_grammar(&self) -> bool {
        false
    }

    /// Rough token count for `text`. Used for budget enforcement.
    fn estimate_tokens(&self, text: &str) -> usize {
        // Default heuristic: roughly 4/3 tokens per whitespace-separated word.
        let words = text.split_whitespace().count();
        words * 4 / 3
    }
}

/// Deterministic mock provider for tests. Returns canned responses or echoes
/// the last user message.
pub mod echo {
    use super::{
        ChatMessage, ChatRole, CompletionRequest, CompletionResponse, CompletionStream,
        FinishReason, LlmProvider, Result, StreamChunk,
    };
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use std::collections::VecDeque;

    /// Mock provider that returns canned responses in order; if exhausted,
    /// echoes the last user message back.
    #[derive(Debug, Default)]
    pub struct EchoLlmProvider {
        canned: Mutex<VecDeque<CompletionResponse>>,
        canned_json: Mutex<VecDeque<serde_json::Value>>,
        canned_streams: Mutex<VecDeque<Vec<StreamChunk>>>,
    }

    impl EchoLlmProvider {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Queue a canned text response.
        pub fn push(&self, text: impl Into<String>) {
            self.canned.lock().push_back(CompletionResponse {
                text: text.into(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
            });
        }

        /// Queue a canned JSON response.
        pub fn push_json(&self, value: serde_json::Value) {
            self.canned_json.lock().push_back(value);
        }

        /// Queue a canned scripted stream. The provider will emit each chunk
        /// in order; the last chunk should be `StreamChunk::Finished`. If
        /// omitted, the trailing `Finished` is appended automatically.
        pub fn push_stream(&self, chunks: Vec<StreamChunk>) {
            let mut chunks = chunks;
            if !chunks
                .iter()
                .any(|c| matches!(c, StreamChunk::Finished { .. }))
            {
                chunks.push(StreamChunk::Finished {
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                });
            }
            self.canned_streams.lock().push_back(chunks);
        }
    }

    #[async_trait]
    impl LlmProvider for EchoLlmProvider {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn model(&self) -> &'static str {
            "echo-1"
        }

        async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
            let canned = self.canned.lock().pop_front();
            if let Some(r) = canned {
                return Ok(r);
            }
            let last_user = req
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, ChatRole::User))
                .cloned()
                .unwrap_or_else(|| ChatMessage::user(String::new()));
            Ok(CompletionResponse {
                text: format!("echo: {}", last_user.content),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
            })
        }

        async fn complete_json(
            &self,
            _req: CompletionRequest,
            _schema: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            let canned = self.canned_json.lock().pop_front();
            if let Some(v) = canned {
                return Ok(v);
            }
            Ok(serde_json::json!({}))
        }

        async fn complete_stream(&self, req: CompletionRequest) -> Result<CompletionStream> {
            let scripted = self.canned_streams.lock().pop_front();
            if let Some(chunks) = scripted {
                let mapped = chunks.into_iter().map(Ok);
                return Ok(Box::pin(futures::stream::iter(mapped)));
            }
            // No scripted stream; fall back to a single-chunk replay of complete().
            let resp = self.complete(req).await?;
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(StreamChunk::Token(resp.text)),
                Ok(StreamChunk::Finished {
                    input_tokens: resp.input_tokens,
                    output_tokens: resp.output_tokens,
                    finish_reason: resp.finish_reason,
                }),
            ])))
        }

        fn supports_streaming(&self) -> bool {
            true
        }
    }
}

pub use echo::EchoLlmProvider;
