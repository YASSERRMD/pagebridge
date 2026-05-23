use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedCandidate {
    pub source_id: String,
    pub node_id: String,
    pub score: f32,
    pub title: String,
}

#[async_trait]
pub trait FederatedSource: Send + Sync + 'static {
    fn source_id(&self) -> &str;
    async fn candidates(&self, query: &str, top_k: usize) -> Vec<FederatedCandidate>;
}
