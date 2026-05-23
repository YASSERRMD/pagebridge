//! Splunk HEC (HTTP Event Collector) sink.
//!
//! Sends each event/batch as a single HEC payload:
//! `{"event": <serialized>, "sourcetype": "pagebridge:audit"}`. Uses the
//! standard `Authorization: Splunk <token>` header.
//!
//! Behind the `http-sinks` feature so the default build remains
//! reqwest-free.

use serde_json::json;

use crate::error::{AuditError, Result};
use crate::event::AuditEvent;
use crate::merkle::MerkleBatch;
use crate::writer::AuditSink;

pub struct SplunkHecSink {
    name: String,
    endpoint: String,
    token: String,
    client: reqwest::Client,
}

impl SplunkHecSink {
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AuditError::Sink {
                sink: "splunk".to_string(),
                message: format!("build client: {e}"),
            })?;
        Ok(Self {
            name: "splunk".to_string(),
            endpoint: endpoint.into(),
            token: token.into(),
            client,
        })
    }

    async fn post(&self, body: serde_json::Value) -> Result<()> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Splunk {}", self.token))
            .json(&body)
            .send()
            .await
            .map_err(|e| AuditError::Sink {
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
impl AuditSink for SplunkHecSink {
    fn name(&self) -> &str {
        &self.name
    }
    async fn write_event(&self, event: &AuditEvent) -> Result<()> {
        self.post(json!({
            "event": event,
            "sourcetype": "pagebridge:audit:event",
        }))
        .await
    }
    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
        self.post(json!({
            "event": batch,
            "sourcetype": "pagebridge:audit:batch",
        }))
        .await
    }
}
