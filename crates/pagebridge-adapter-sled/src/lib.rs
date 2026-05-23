//! Sled embedded KV adapter for pagebridge.
//!
//! Dialect: Lock-free B+tree.
//!
//! The default build ships a typed scaffold; the full driver-backed
//! implementation lives behind the `driver` feature so consumers do
//! not pay the build cost when they only need the trait shape.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use async_trait::async_trait;

use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

pub struct Adapter {
    url: String,
}

impl Adapter {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

fn unimpl<T>(adapter: &'static str) -> Result<T> {
    Err(PagebridgeError::Adapter {
        adapter: adapter.into(),
        message: "configure the `driver` feature to use this adapter".into(),
    })
}

#[async_trait]
impl StorageAdapter for Adapter {
    fn name(&self) -> &'static str {
        "sled"
    }

    async fn migrate(&self) -> Result<()> {
        Ok(())
    }

    async fn upsert_node(&self, _node: &NodeRecord) -> Result<()> {
        unimpl("sled")
    }

    async fn get_node(&self, _id: &NodeId) -> Result<Option<NodeRecord>> {
        unimpl("sled")
    }

    async fn get_nodes(&self, _ids: &[NodeId]) -> Result<Vec<NodeRecord>> {
        unimpl("sled")
    }

    async fn children_summaries(&self, _parent: &NodeId) -> Result<Vec<NodeSummary>> {
        unimpl("sled")
    }

    async fn children_records(&self, _parent: &NodeId) -> Result<Vec<NodeRecord>> {
        unimpl("sled")
    }

    async fn path_to(&self, _id: &NodeId) -> Result<Vec<NodeRecord>> {
        unimpl("sled")
    }

    async fn leaves_under(&self, _root: &NodeId) -> Result<Vec<NodeId>> {
        unimpl("sled")
    }

    async fn delete_document(&self, _doc_id: &DocId) -> Result<()> {
        unimpl("sled")
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        unimpl("sled")
    }

    async fn upsert_document(&self, _doc: &DocumentEntry) -> Result<()> {
        unimpl("sled")
    }

    async fn bm25_search(&self, _query: &str, _limit: usize) -> Result<Vec<SearchHit>> {
        unimpl("sled")
    }

    async fn bm25_search_in_doc(
        &self,
        _doc_id: &DocId,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SearchHit>> {
        unimpl("sled")
    }

    async fn put_raw(&self, _doc_id: &DocId, _data: &[u8]) -> Result<u64> {
        unimpl("sled")
    }

    async fn read_raw_span(&self, _doc_id: &DocId, _span: (u64, u64)) -> Result<Vec<u8>> {
        unimpl("sled")
    }

    async fn get_summary_cache(&self, _hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        Ok(None)
    }

    async fn upsert_summary_cache(
        &self,
        _hash: &[u8; 32],
        _entry: &SummaryCacheEntry,
    ) -> Result<()> {
        Ok(())
    }

    async fn stats(&self) -> Result<AdapterStats> {
        Ok(AdapterStats::default())
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_round_trip() {
        let a = Adapter::new("test://x");
        assert_eq!(a.url(), "test://x");
        assert_eq!(a.name(), "sled");
    }
}
