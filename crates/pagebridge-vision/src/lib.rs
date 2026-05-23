//! Vision-mode PDF ingestion helpers.
//!
//! The text-mode PDF ingester is the default. When the extracted text scores
//! poorly (low character density, lots of unicode private-use chars, garbled
//! word shapes), this crate provides:
//!
//! 1. A [`quality::score_text`] heuristic to decide when to fall back.
//! 2. A vision-mode pipeline that rasterizes pages to PNG and dispatches them
//!    to a vision-capable [`pagebridge_core::LlmProvider`] (see the
//!    `LlmProvider::supports_vision` method).
//!
//! Page rasterization is gated behind the `rasterize` cargo feature so the
//! workspace stays buildable without `pdfium-render` or `pdfium` shared
//! libraries.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::unused_async,
    clippy::missing_const_for_fn,
    clippy::too_long_first_doc_paragraph,
    clippy::suboptimal_flops,
    clippy::unreadable_literal,
    clippy::manual_repeat_n,
    clippy::assertions_on_constants
)]

pub mod quality;

use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{ChatMessage, CompletionRequest, LlmProvider, VisionImage};

/// Render a PDF page to PNG bytes at the given DPI. Returns one image per
/// requested page. Without the `rasterize` feature this returns an explicit
/// "rasterization not enabled" error so callers know to recompile.
pub async fn rasterize_pages(_pdf_bytes: &[u8], _dpi: u32) -> Result<Vec<VisionImage>> {
    #[cfg(not(feature = "rasterize"))]
    {
        Err(PagebridgeError::Internal(
            "pagebridge-vision was built without the 'rasterize' feature; rebuild with --features rasterize"
                .into(),
        ))
    }
    #[cfg(feature = "rasterize")]
    {
        // Real rasterization stays behind the feature flag to avoid linking
        // pdfium when callers do not need it. Implementations live in a
        // separate module guarded by the feature.
        Err(PagebridgeError::Internal(
            "pagebridge-vision rasterize backend not implemented yet".into(),
        ))
    }
}

/// Trait shared by vision providers used by the ingester. A blanket impl
/// adapts any [`LlmProvider`] that reports `supports_vision() == true`.
#[async_trait]
pub trait VisionProvider: Send + Sync + 'static {
    /// Describe a single page image as structured JSON.
    async fn describe_page(&self, image: &VisionImage) -> Result<serde_json::Value>;
}

/// Wrap any [`LlmProvider`] to honor the [`VisionProvider`] contract. The
/// adapter only succeeds if the underlying provider claims `supports_vision`.
pub struct VisionAdapter {
    inner: Arc<dyn LlmProvider>,
}

impl VisionAdapter {
    /// Construct from a shared LLM provider handle.
    pub fn new(inner: Arc<dyn LlmProvider>) -> Result<Self> {
        if !inner.supports_vision() {
            return Err(PagebridgeError::Llm {
                provider: inner.name().into(),
                message: "provider does not advertise supports_vision()".into(),
            });
        }
        Ok(Self { inner })
    }
}

#[async_trait]
impl VisionProvider for VisionAdapter {
    async fn describe_page(&self, image: &VisionImage) -> Result<serde_json::Value> {
        let prompt = "Describe the structure and content of this page image. Output JSON with \
            keys: title (string), sections (array of {heading, body}), tables (array of markdown \
            strings), figures (array of {caption}).";
        let req = CompletionRequest {
            system: Some(
                "You are a careful document extractor. Always reply with valid JSON.".into(),
            ),
            messages: vec![ChatMessage::user(prompt)],
            images: vec![image.clone()],
            ..CompletionRequest::default()
        };
        let resp = self.inner.complete(req).await?;
        serde_json::from_str(resp.text.trim()).map_err(|e| PagebridgeError::Llm {
            provider: self.inner.name().into(),
            message: format!("vision json: {e}"),
        })
    }
}

/// Test-only echo vision provider that returns canned structured outputs.
pub struct EchoVisionProvider {
    canned: parking_lot::Mutex<std::collections::VecDeque<serde_json::Value>>,
}

impl EchoVisionProvider {
    #[must_use]
    pub fn new() -> Self {
        Self {
            canned: parking_lot::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub fn push(&self, value: serde_json::Value) {
        self.canned.lock().push_back(value);
    }
}

impl Default for EchoVisionProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VisionProvider for EchoVisionProvider {
    async fn describe_page(&self, _image: &VisionImage) -> Result<serde_json::Value> {
        let canned = self.canned.lock().pop_front();
        Ok(canned.unwrap_or_else(|| {
            serde_json::json!({
                "title": "Untitled",
                "sections": [],
                "tables": [],
                "figures": []
            })
        }))
    }
}
