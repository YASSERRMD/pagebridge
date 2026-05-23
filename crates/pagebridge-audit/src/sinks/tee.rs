//! TeeSink: a sink that fans every event/batch out to a static set of
//! inner sinks. Used to chain a fast local file sink with one or more
//! durable external sinks without making the writer aware of the list.

use std::sync::Arc;

use crate::error::Result;
use crate::event::AuditEvent;
use crate::merkle::MerkleBatch;
use crate::writer::AuditSink;

pub struct TeeSink {
    name: String,
    inner: Vec<Arc<dyn AuditSink>>,
}

impl TeeSink {
    pub fn new(name: impl Into<String>, inner: Vec<Arc<dyn AuditSink>>) -> Self {
        Self {
            name: name.into(),
            inner,
        }
    }
}

#[async_trait::async_trait]
impl AuditSink for TeeSink {
    fn name(&self) -> &str {
        &self.name
    }
    async fn write_event(&self, event: &AuditEvent) -> Result<()> {
        for s in &self.inner {
            s.write_event(event).await?;
        }
        Ok(())
    }
    async fn write_batch(&self, batch: &MerkleBatch) -> Result<()> {
        for s in &self.inner {
            s.write_batch(batch).await?;
        }
        Ok(())
    }
}
