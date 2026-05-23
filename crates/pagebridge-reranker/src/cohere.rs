//! Cohere reranker (rerank-3.0 family).

use async_trait::async_trait;
use serde_json::json;

use crate::{RerankedDoc, Reranker, RerankerError, Result};

pub struct CohereReranker {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl CohereReranker {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| RerankerError::Provider {
                provider: "cohere".into(),
                message: format!("client: {e}"),
            })?;
        Ok(Self {
            api_key: api_key.into(),
            model: model.into(),
            client,
        })
    }
}

#[async_trait]
impl Reranker for CohereReranker {
    fn name(&self) -> &'static str {
        "cohere"
    }
    fn model(&self) -> &str {
        &self.model
    }
    async fn rerank(
        &self,
        query: &str,
        docs: &[String],
        top_k: usize,
    ) -> Result<Vec<RerankedDoc>> {
        let body = json!({
            "model": self.model,
            "query": query,
            "documents": docs,
            "top_n": top_k,
        });
        let resp = self
            .client
            .post("https://api.cohere.com/v2/rerank")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| RerankerError::Provider {
                provider: "cohere".into(),
                message: format!("send: {e}"),
            })?;
        if !resp.status().is_success() {
            return Err(RerankerError::Provider {
                provider: "cohere".into(),
                message: format!("HTTP {}", resp.status()),
            });
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| RerankerError::Provider {
            provider: "cohere".into(),
            message: format!("parse: {e}"),
        })?;
        let arr = v["results"]
            .as_array()
            .ok_or_else(|| RerankerError::Provider {
                provider: "cohere".into(),
                message: "missing results[]".into(),
            })?;
        let mut out = Vec::with_capacity(arr.len());
        for r in arr {
            let index = r["index"].as_u64().unwrap_or(0) as usize;
            let score = r["relevance_score"].as_f64().unwrap_or(0.0) as f32;
            out.push(RerankedDoc { index, score });
        }
        Ok(out)
    }
}
