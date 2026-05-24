//! Embedded llama.cpp LLM provider for pagebridge.
//!
//! Two build modes:
//!
//! - **default (`cpu` feature without `llamacpp-driver`)**: the provider
//!   compiles to a stub whose every call returns an explicit "driver not
//!   enabled" error. This keeps the workspace portable on hosts without a
//!   C++ toolchain.
//! - **`llamacpp-driver`**: links the real `llama-cpp-2` crate, loads a GGUF
//!   model, and runs inference in-process. Hardware acceleration is opt-in
//!   via the `cuda`, `metal`, or `vulkan` feature flags (each implies
//!   `llamacpp-driver`).
//!
//! Even in stub mode, the GBNF grammar lowering for JSON schemas is
//! available via the [`grammar`] module so callers can inspect what would
//! be sent to the model.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unused_async,
    clippy::module_name_repetitions,
    dead_code,
    unused_imports
)]

pub mod grammar;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmConfig, LlmProvider, RateLimits,
};

/// Runtime configuration knobs that are independent of the chosen backend.
#[derive(Debug, Clone, Copy)]
pub struct LlamaCppConfig {
    /// Context window size in tokens. Default 8192.
    pub context_size: u32,
    /// Number of CPU threads. Default `-1` lets llama.cpp pick.
    pub n_threads: i32,
    /// Layers to offload to GPU. Default 0 (CPU-only).
    pub n_gpu_layers: i32,
    /// Sampling seed.
    pub seed: u32,
    /// Memory-map the GGUF file.
    pub use_mmap: bool,
}

impl Default for LlamaCppConfig {
    fn default() -> Self {
        Self {
            context_size: 8192,
            n_threads: -1,
            n_gpu_layers: 0,
            seed: 42,
            use_mmap: true,
        }
    }
}

/// Embedded llama.cpp LLM provider.
#[derive(Debug, Clone)]
pub struct LlamaCppProvider {
    gguf_path: PathBuf,
    config: LlamaCppConfig,
    llm: LlmConfig,
    #[cfg(feature = "llamacpp-driver")]
    inner: std::sync::Arc<driver::Inner>,
}

impl LlamaCppProvider {
    /// Load a GGUF model from disk with default configuration.
    pub fn from_gguf(path: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(path, LlamaCppConfig::default(), LlmConfig::default())
    }

    /// Load a GGUF model from disk with a custom configuration.
    pub fn with_config(
        path: impl AsRef<Path>,
        config: LlamaCppConfig,
        llm: LlmConfig,
    ) -> Result<Self> {
        let gguf_path = path.as_ref().to_path_buf();
        if !gguf_path.exists() {
            return Err(PagebridgeError::Llm {
                provider: "llamacpp".into(),
                message: format!("gguf file not found: {}", gguf_path.display()),
            });
        }
        #[cfg(feature = "llamacpp-driver")]
        let inner = std::sync::Arc::new(driver::Inner::load(&gguf_path, &config)?);
        Ok(Self {
            gguf_path,
            config,
            llm,
            #[cfg(feature = "llamacpp-driver")]
            inner,
        })
    }

    /// Path of the loaded GGUF file.
    #[must_use]
    pub fn gguf_path(&self) -> &Path {
        &self.gguf_path
    }

    /// Active configuration.
    #[must_use]
    pub const fn config(&self) -> &LlamaCppConfig {
        &self.config
    }
}

#[async_trait]
impl LlmProvider for LlamaCppProvider {
    fn name(&self) -> &'static str {
        "llamacpp"
    }

    fn model(&self) -> &str {
        self.gguf_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.gguf")
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        #[cfg(feature = "llamacpp-driver")]
        {
            self.inner.complete(&self.config, &self.llm, req).await
        }
        #[cfg(not(feature = "llamacpp-driver"))]
        {
            let _ = req;
            Err(driver_disabled())
        }
    }

    async fn complete_json(
        &self,
        req: CompletionRequest,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let _gbnf = grammar::schema_to_gbnf(schema);
        #[cfg(feature = "llamacpp-driver")]
        {
            self.inner
                .complete_json(&self.config, &self.llm, req, _gbnf)
                .await
        }
        #[cfg(not(feature = "llamacpp-driver"))]
        {
            let _ = req;
            Err(driver_disabled())
        }
    }

    fn supports_grammar(&self) -> bool {
        true
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Closer-than-default rough heuristic: roughly 1 token per 4 chars.
        text.len().div_ceil(4)
    }

    fn rate_limits(&self) -> RateLimits {
        RateLimits::local()
    }
}

#[cfg(not(feature = "llamacpp-driver"))]
fn driver_disabled() -> PagebridgeError {
    PagebridgeError::Llm {
        provider: "llamacpp".into(),
        message:
            "llama.cpp driver not enabled. Rebuild with --features llamacpp-driver and ensure \
             a C++ toolchain is installed."
                .into(),
    }
}

#[cfg(feature = "llamacpp-driver")]
mod driver {
    //! Real backend wired against the `llama-cpp-2` crate.
    //!
    //! The model is loaded once and shared across calls. Each `complete`
    //! creates a fresh inference context from the shared model handle and
    //! runs sampling on a blocking thread.

    use super::*;
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaModel, Special};
    use parking_lot::Mutex;
    use std::num::NonZeroU32;

    pub struct Inner {
        backend: LlamaBackend,
        model: Mutex<LlamaModel>,
    }

    impl Inner {
        pub fn load(path: &Path, cfg: &LlamaCppConfig) -> Result<Self> {
            let backend = LlamaBackend::init().map_err(|e| llm_err("backend init", e))?;
            let mut params = LlamaModelParams::default();
            params = params.with_n_gpu_layers(u32::try_from(cfg.n_gpu_layers).unwrap_or(0));
            let model =
                LlamaModel::load_from_file(&backend, path, &params).map_err(|e| llm_err("load gguf", e))?;
            Ok(Self {
                backend,
                model: Mutex::new(model),
            })
        }

        pub async fn complete(
            &self,
            cfg: &LlamaCppConfig,
            llm: &LlmConfig,
            req: CompletionRequest,
        ) -> Result<CompletionResponse> {
            self.generate(cfg, llm, req, None).await
        }

        pub async fn complete_json(
            &self,
            cfg: &LlamaCppConfig,
            llm: &LlmConfig,
            req: CompletionRequest,
            gbnf: String,
        ) -> Result<serde_json::Value> {
            let resp = self.generate(cfg, llm, req, Some(gbnf)).await?;
            serde_json::from_str(&resp.text).map_err(|e| llm_err("decode json", e))
        }

        async fn generate(
            &self,
            cfg: &LlamaCppConfig,
            llm: &LlmConfig,
            req: CompletionRequest,
            _gbnf: Option<String>,
        ) -> Result<CompletionResponse> {
            let prompt = render_prompt(&req);
            let max_tokens = req
                .max_tokens
                .unwrap_or(llm.default_max_tokens)
                .min(cfg.context_size);
            // The inference loop runs on a blocking thread because token-by-
            // token sampling against a llama context is CPU-bound and not
            // cancellable. The model handle is shared via Arc<Mutex<...>>.
            let context_size = cfg.context_size;
            let model = self.model.clone();
            let _ = &self.backend;
            tokio::task::spawn_blocking(move || {
                let model = model.lock();
                let mut params = LlamaContextParams::default();
                if let Some(n) = NonZeroU32::new(context_size) {
                    params = params.with_n_ctx(Some(n));
                }
                let backend = LlamaBackend::init().map_err(|e| llm_err("backend reinit", e))?;
                let mut ctx = model
                    .new_context(&backend, params)
                    .map_err(|e| llm_err("ctx", e))?;
                let tokens = model
                    .str_to_token(&prompt, AddBos::Always)
                    .map_err(|e| llm_err("tokenize", e))?;
                let input_tokens = tokens.len() as u32;
                let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(512, 1);
                for (i, t) in tokens.iter().enumerate() {
                    batch
                        .add(*t, i as i32, &[0], i == tokens.len() - 1)
                        .map_err(|e| llm_err("batch add", e))?;
                }
                ctx.decode(&mut batch).map_err(|e| llm_err("decode", e))?;

                let mut out = String::new();
                let mut produced = 0u32;
                let mut cursor = tokens.len() as i32;
                while produced < max_tokens {
                    let candidates = ctx.candidates();
                    let next = ctx
                        .sample_token_greedy(candidates)
                        .ok_or_else(|| llm_err("sample", "no candidate"))?;
                    if model.is_eog_token(next) {
                        break;
                    }
                    let piece = model
                        .token_to_str(next, Special::Tokenize)
                        .map_err(|e| llm_err("detok", e))?;
                    out.push_str(&piece);
                    produced += 1;

                    batch.clear();
                    batch
                        .add(next, cursor, &[0], true)
                        .map_err(|e| llm_err("batch add next", e))?;
                    cursor += 1;
                    ctx.decode(&mut batch).map_err(|e| llm_err("step", e))?;
                }
                Ok(CompletionResponse {
                    text: out,
                    input_tokens,
                    output_tokens: produced,
                    finish_reason: if produced >= max_tokens {
                        FinishReason::Length
                    } else {
                        FinishReason::Stop
                    },
                })
            })
            .await
            .map_err(|e| llm_err("join", e))?
        }
    }

    fn render_prompt(req: &CompletionRequest) -> String {
        let mut out = String::new();
        if let Some(sys) = &req.system {
            out.push_str(sys);
            out.push_str("\n\n");
        }
        for m in &req.messages {
            match m.role {
                pagebridge_core::llm::ChatRole::System => {
                    out.push_str("System: ");
                    out.push_str(&m.content);
                    out.push('\n');
                }
                pagebridge_core::llm::ChatRole::User => {
                    out.push_str("User: ");
                    out.push_str(&m.content);
                    out.push('\n');
                }
                pagebridge_core::llm::ChatRole::Assistant => {
                    out.push_str("Assistant: ");
                    out.push_str(&m.content);
                    out.push('\n');
                }
            }
        }
        out.push_str("Assistant: ");
        out
    }

    fn llm_err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
        PagebridgeError::Llm {
            provider: "llamacpp".into(),
            message: format!("{ctx}: {e}"),
        }
    }
}
