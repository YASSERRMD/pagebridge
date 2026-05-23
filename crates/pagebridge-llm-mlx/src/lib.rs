//! Apple Silicon on-device provider backed by MLX.
//!
//! MLX is Apple's array framework with first-class Metal acceleration on
//! M-series Macs. Pagebridge uses it for fully on-device inference on
//! iOS/macOS deployments. The native bridge is feature-gated (the `mlx`
//! crate requires xcode-cli-tools); the default build returns a typed
//! error so the provider can still be wired into the umbrella crate
//! without paying the build cost.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use async_trait::async_trait;

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{CompletionRequest, CompletionResponse, LlmProvider};

#[derive(Debug, Clone)]
pub struct MlxProvider {
    model_path: String,
    model: String,
}

impl MlxProvider {
    pub fn new(model_path: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            model: model.into(),
        }
    }

    #[must_use]
    pub fn model_path(&self) -> &str {
        &self.model_path
    }
}

#[async_trait]
impl LlmProvider for MlxProvider {
    fn name(&self) -> &'static str {
        "mlx"
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Err(PagebridgeError::Llm {
            provider: "mlx".into(),
            message: "MlxProvider requires the `mlx` feature and Apple Silicon".into(),
        })
    }

    async fn complete_json(
        &self,
        _req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Err(PagebridgeError::Llm {
            provider: "mlx".into(),
            message: "MlxProvider requires the `mlx` feature".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_path_round_trip() {
        let p = MlxProvider::new("/tmp/m", "qwen2.5-1.5b-mlx");
        assert_eq!(p.name(), "mlx");
        assert_eq!(p.model_path(), "/tmp/m");
    }
}
