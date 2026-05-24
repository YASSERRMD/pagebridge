//! Elasticsearch / OpenSearch sink.
//!
//! Posts each event to `<endpoint>/<index_events>/_doc` and each batch to
//! `<endpoint>/<index_batches>/_doc`. Uses optional basic auth.

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;
use crate::merkle::MerkleBatch;
use crate::writer::AuditSink;

pub struct ElasticSink {
    name: String,
    endpoint: String,
    index_events: String,
    index_batches: String,
    auth: Option<(String, String)>,
    client: reqwest::Client,
}

impl ElasticSink {
    pub fn new(endpoint: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AuditError::Sink {
                sink: "elastic".to_string(),
                message: format!("build client: {e}"),
            })?;
        Ok(Self {
            name: "elastic".to_string(),
            endpoint: endpoint.into(),
            index_events: "pagebridge-audit-events".to_string(),
            index_batches: "pagebridge-audit-batches".to_string(),
            auth: None,
            client,
        })
    }

    #[must_use]
    pub fn with_basic_auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.auth = Some((user.into(), pass.into()));
        self
    }

    #[must_use]
    pub fn with_indexes(mut self, events: impl Into<String>, batches: impl Into<String>) -> Self {
        self.index_events = events.into();
        self.index_batches = batches.into();
        self
    }

    async fn post<T: serde::Serialize>(&self, index: &str, body: &T) -> Result<()> {
        let url = format!("{}/{}/_doc", self.endpoint.trim_end_matches('/'), index);
        let mut req = self.client.post(&url).json(body);
        if let Some((user, pass)) = &self.auth {
            req = req.basic_auth(user, Some(pass));
        }
        let resp = req.send().await.map_err(|e| AuditError::Sink {
            sink: self.name.clone(),
            message: format!("send: {e}"),
        })?;
        if !resp.status().is_success() {
            return Err(AuditError::Sink {
                sink: self.name.clone(),
                message: format!("HTTP {}", resp.status()),
            });
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AuditSink for ElasticSink {
    fn name(&self) -> &str {
        &self.name
    }
    async fn write_event(&self, event: &AuditEvent) -> Result<()> {
        self.post(&self.index_events.clone(), event).await
    }
    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
        self.post(&self.index_batches.clone(), batch).await
    }
}
