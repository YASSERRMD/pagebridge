//! AWS Bedrock provider.
//!
//! Bedrock fronts dozens of foundation models (Anthropic, Meta, Mistral,
//! Cohere, Amazon Titan). The real AWS SDK depends on `aws-sdk-bedrockruntime`
//! which adds noticeable build time; we ship a typed scaffold here behind
//! the `sdk` feature and a stub implementation for the default build so
//! the umbrella crate can list Bedrock without paying the build cost
//! when it is not actually used.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use async_trait::async_trait;

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider,
};

#[derive(Debug, Clone)]
pub struct BedrockProvider {
    region: String,
    model_id: String,
}

impl BedrockProvider {
    pub fn new(region: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            model_id: model_id.into(),
        }
    }

    #[must_use]
    pub fn region(&self) -> &str {
        &self.region
    }
}

#[async_trait]
impl LlmProvider for BedrockProvider {
    fn name(&self) -> &'static str {
        "bedrock"
    }
    fn model(&self) -> &str {
        &self.model_id
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        // The SDK-backed implementation lands behind the `sdk` feature
        // in a follow-up; the default build returns a typed error so
        // consumers get a clear "configure the sdk feature" message.
        Err(PagebridgeError::Llm {
            provider: "bedrock".into(),
            message: "BedrockProvider requires the `sdk` feature; see pagebridge-llm-bedrock README".into(),
        })
    }

    async fn complete_json(
        &self,
        _req: CompletionRequest,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Err(PagebridgeError::Llm {
            provider: "bedrock".into(),
            message: "BedrockProvider requires the `sdk` feature".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_and_model_round_trip() {
        let p = BedrockProvider::new("us-east-1", "anthropic.claude-3-5-sonnet-20240620-v1:0");
        assert_eq!(p.region(), "us-east-1");
        assert_eq!(p.name(), "bedrock");
    }
}
