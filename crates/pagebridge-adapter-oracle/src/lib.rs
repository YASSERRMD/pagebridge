//! Oracle Database storage adapter for pagebridge.
//!
//! The underlying `oracle` Rust crate is a synchronous wrapper around Oracle
//! Instant Client. To avoid blocking the tokio runtime, every call to the
//! driver is dispatched through `tokio::task::spawn_blocking`. A hand-rolled
//! connection pool guards a `Vec<Connection>` behind a `parking_lot::Mutex`,
//! since `bb8`-style pools assume async drivers.
//!
//! The crate compiles in two modes:
//! - default: a stub that surfaces an explicit "oracle driver not enabled"
//!   error from every constructor. The workspace builds without the Oracle
//!   Instant Client SDK.
//! - `oracle-driver`: real driver enabled. Requires Oracle Instant Client on
//!   the build host (`libclntsh`).
//!
//! See `docs/ADAPTERS.md` for Instant Client installation notes.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::wildcard_imports,
    clippy::unused_async,
    unused_imports
)]

use async_trait::async_trait;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

#[cfg(feature = "oracle-driver")]
pub(crate) const RAW_CHUNK_LIMIT: usize = 1024 * 1024;

#[cfg(feature = "oracle-driver")]
pub(crate) const fn level_to_i32(l: NodeLevel) -> i32 {
    match l {
        NodeLevel::Corpus => 0,
        NodeLevel::Document => 1,
        NodeLevel::Section => 2,
        NodeLevel::Subsection => 3,
        NodeLevel::Page => 4,
        NodeLevel::Leaf => 5,
    }
}

#[cfg(feature = "oracle-driver")]
pub(crate) const fn level_from_i32(v: i32) -> NodeLevel {
    match v {
        0 => NodeLevel::Corpus,
        1 => NodeLevel::Document,
        2 => NodeLevel::Section,
        3 => NodeLevel::Subsection,
        4 => NodeLevel::Page,
        _ => NodeLevel::Leaf,
    }
}

#[cfg(feature = "oracle-driver")]
pub(crate) fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "oracle".into(),
        message: format!("{ctx}: {e}"),
    }
}

#[cfg(feature = "oracle-driver")]
mod ops;
#[cfg(feature = "oracle-driver")]
mod pool;
#[cfg(feature = "oracle-driver")]
mod schema;

#[cfg(feature = "oracle-driver")]
pub use real::OracleAdapter;
#[cfg(not(feature = "oracle-driver"))]
pub use stub::OracleAdapter;

#[cfg(feature = "oracle-driver")]
mod real {
    use super::*;
    use crate::pool::OraclePool;

    /// Oracle Database storage adapter.
    ///
    /// Wraps a hand-rolled pool of `oracle::Connection`s. Every driver call is
    /// dispatched onto `tokio::task::spawn_blocking` because the underlying
    /// driver is synchronous.
    #[derive(Clone)]
    pub struct OracleAdapter {
        pub(crate) pool: OraclePool,
    }

    impl std::fmt::Debug for OracleAdapter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("OracleAdapter").finish_non_exhaustive()
        }
    }

    impl OracleAdapter {
        /// Connect with explicit credentials.
        pub async fn connect(username: &str, password: &str, connect_string: &str) -> Result<Self> {
            let pool = OraclePool::new(username, password, connect_string, 4)?;
            let adapter = Self { pool };
            adapter.ping().await?;
            Ok(adapter)
        }
    }

    #[async_trait]
    impl StorageAdapter for OracleAdapter {
        fn name(&self) -> &'static str {
            "oracle"
        }
        async fn migrate(&self) -> Result<()> {
            crate::schema::create(&self.pool).await
        }
        async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
            crate::ops::upsert_node(&self.pool, node).await
        }
        async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
            crate::ops::get_node(&self.pool, id).await
        }
        async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodeRecord>> {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(n) = self.get_node(id).await? {
                    out.push(n);
                }
            }
            Ok(out)
        }
        async fn children_summaries(&self, parent: &NodeId) -> Result<Vec<NodeSummary>> {
            crate::ops::children_summaries(&self.pool, parent).await
        }
        async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
            crate::ops::children_records(&self.pool, parent).await
        }
        async fn path_to(&self, id: &NodeId) -> Result<Vec<NodeRecord>> {
            let mut chain = Vec::new();
            let mut cursor: Option<NodeId> = Some(id.clone());
            while let Some(c) = cursor.take() {
                let Some(rec) = self.get_node(&c).await? else {
                    break;
                };
                cursor.clone_from(&rec.parent_id);
                chain.push(rec);
            }
            chain.reverse();
            Ok(chain)
        }
        async fn leaves_under(&self, root: &NodeId) -> Result<Vec<NodeId>> {
            crate::ops::leaves_under(&self.pool, root).await
        }
        async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
            crate::ops::delete_document(&self.pool, doc_id).await
        }
        async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
            crate::ops::list_documents(&self.pool).await
        }
        async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
            crate::ops::upsert_document(&self.pool, doc).await
        }
        async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
            crate::ops::search(&self.pool, query, limit, None).await
        }
        async fn bm25_search_in_doc(
            &self,
            doc_id: &DocId,
            query: &str,
            limit: usize,
        ) -> Result<Vec<SearchHit>> {
            crate::ops::search(&self.pool, query, limit, Some(doc_id)).await
        }
        async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
            crate::ops::put_raw(&self.pool, doc_id, data, RAW_CHUNK_LIMIT).await
        }
        async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
            crate::ops::read_raw_span(&self.pool, doc_id, span).await
        }
        async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
            crate::ops::get_summary_cache(&self.pool, hash).await
        }
        async fn upsert_summary_cache(
            &self,
            hash: &[u8; 32],
            entry: &SummaryCacheEntry,
        ) -> Result<()> {
            crate::ops::upsert_summary_cache(&self.pool, hash, entry).await
        }
        async fn stats(&self) -> Result<AdapterStats> {
            crate::ops::stats(&self.pool).await
        }
        async fn ping(&self) -> Result<()> {
            crate::ops::ping(&self.pool).await
        }
    }
}

#[cfg(not(feature = "oracle-driver"))]
mod stub {
    use super::*;

    /// Stub Oracle adapter, available when the `oracle-driver` feature is off.
    ///
    /// Every method returns an error explaining that the Oracle Instant Client
    /// SDK was not enabled at build time. Useful for keeping the workspace
    /// buildable on hosts without Oracle libs.
    #[derive(Debug, Clone)]
    pub struct OracleAdapter;

    impl OracleAdapter {
        pub async fn connect(
            _username: &str,
            _password: &str,
            _connect_string: &str,
        ) -> Result<Self> {
            Err(driver_disabled())
        }
    }

    fn driver_disabled() -> PagebridgeError {
        PagebridgeError::Adapter {
            adapter: "oracle".into(),
            message: "Oracle driver not enabled. Rebuild with --features oracle-driver and ensure \
                 Oracle Instant Client is installed."
                .into(),
        }
    }

    #[async_trait]
    impl StorageAdapter for OracleAdapter {
        fn name(&self) -> &'static str {
            "oracle"
        }
        async fn migrate(&self) -> Result<()> {
            Err(driver_disabled())
        }
        async fn upsert_node(&self, _n: &NodeRecord) -> Result<()> {
            Err(driver_disabled())
        }
        async fn get_node(&self, _id: &NodeId) -> Result<Option<NodeRecord>> {
            Err(driver_disabled())
        }
        async fn get_nodes(&self, _ids: &[NodeId]) -> Result<Vec<NodeRecord>> {
            Err(driver_disabled())
        }
        async fn children_summaries(&self, _p: &NodeId) -> Result<Vec<NodeSummary>> {
            Err(driver_disabled())
        }
        async fn children_records(&self, _p: &NodeId) -> Result<Vec<NodeRecord>> {
            Err(driver_disabled())
        }
        async fn path_to(&self, _id: &NodeId) -> Result<Vec<NodeRecord>> {
            Err(driver_disabled())
        }
        async fn leaves_under(&self, _r: &NodeId) -> Result<Vec<NodeId>> {
            Err(driver_disabled())
        }
        async fn delete_document(&self, _d: &DocId) -> Result<()> {
            Err(driver_disabled())
        }
        async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
            Err(driver_disabled())
        }
        async fn upsert_document(&self, _d: &DocumentEntry) -> Result<()> {
            Err(driver_disabled())
        }
        async fn bm25_search(&self, _q: &str, _l: usize) -> Result<Vec<SearchHit>> {
            Err(driver_disabled())
        }
        async fn bm25_search_in_doc(
            &self,
            _d: &DocId,
            _q: &str,
            _l: usize,
        ) -> Result<Vec<SearchHit>> {
            Err(driver_disabled())
        }
        async fn put_raw(&self, _d: &DocId, _data: &[u8]) -> Result<u64> {
            Err(driver_disabled())
        }
        async fn read_raw_span(&self, _d: &DocId, _s: (u64, u64)) -> Result<Vec<u8>> {
            Err(driver_disabled())
        }
        async fn get_summary_cache(&self, _h: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
            Err(driver_disabled())
        }
        async fn upsert_summary_cache(&self, _h: &[u8; 32], _e: &SummaryCacheEntry) -> Result<()> {
            Err(driver_disabled())
        }
        async fn stats(&self) -> Result<AdapterStats> {
            Err(driver_disabled())
        }
        async fn ping(&self) -> Result<()> {
            Err(driver_disabled())
        }
    }
}
