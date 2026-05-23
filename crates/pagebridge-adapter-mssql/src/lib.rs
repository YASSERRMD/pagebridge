//! Microsoft SQL Server storage adapter for pagebridge.
//!
//! Uses `tiberius` over a `bb8` connection pool. Full-text search uses SQL Server
//! Full-Text indexes via `CONTAINSTABLE`, with scores returned in the
//! "higher is more relevant" convention.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]

use async_trait::async_trait;
use bb8::Pool;
use bb8_tiberius::ConnectionManager;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use tiberius::Config;

pub(crate) const RAW_CHUNK_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) type MsPool = Pool<ConnectionManager>;

/// SQL Server storage adapter, backed by a `bb8` pool of `tiberius` connections.
#[derive(Clone)]
pub struct MSSqlAdapter {
    pool: MsPool,
}

impl std::fmt::Debug for MSSqlAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MSSqlAdapter").finish_non_exhaustive()
    }
}

impl MSSqlAdapter {
    /// Connect using an ADO.NET-style connection string, e.g.
    /// `server=tcp:127.0.0.1,1433;user=sa;password=Strong!Passw0rd;database=test;TrustServerCertificate=true`.
    pub async fn from_ado_string(conn_str: &str) -> Result<Self> {
        let config = Config::from_ado_string(conn_str).map_err(|e| err("parse ado string", e))?;
        Self::from_config(config).await
    }

    /// Connect using a fully-formed `tiberius::Config`.
    pub async fn from_config(config: Config) -> Result<Self> {
        let manager = ConnectionManager::new(config);
        let pool = Pool::builder()
            .max_size(16)
            .build(manager)
            .await
            .map_err(|e| err("build pool", e))?;
        let adapter = Self { pool };
        adapter.ping().await?;
        Ok(adapter)
    }

    /// Access the inner bb8 pool. Mainly for test fixtures.
    #[must_use]
    pub const fn pool(&self) -> &MsPool {
        &self.pool
    }
}

pub(crate) fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "mssql".into(),
        message: format!("{ctx}: {e}"),
    }
}

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

#[async_trait]
impl StorageAdapter for MSSqlAdapter {
    fn name(&self) -> &'static str {
        "mssql"
    }

    async fn migrate(&self) -> Result<()> {
        schema::create(&self.pool).await
    }

    async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
        ops::upsert_node(&self.pool, node).await
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        ops::get_node(&self.pool, id).await
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
        ops::children_summaries(&self.pool, parent).await
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        ops::children_records(&self.pool, parent).await
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
        ops::leaves_under(&self.pool, root).await
    }

    async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
        ops::delete_document(&self.pool, doc_id).await
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        ops::list_documents(&self.pool).await
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        ops::upsert_document(&self.pool, doc).await
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        ops::search(&self.pool, query, limit, None).await
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        ops::search(&self.pool, query, limit, Some(doc_id)).await
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        ops::put_raw(&self.pool, doc_id, data, RAW_CHUNK_LIMIT).await
    }

    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
        ops::read_raw_span(&self.pool, doc_id, span).await
    }

    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        ops::get_summary_cache(&self.pool, hash).await
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        ops::upsert_summary_cache(&self.pool, hash, entry).await
    }

    async fn stats(&self) -> Result<AdapterStats> {
        ops::stats(&self.pool).await
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self.pool.get().await.map_err(|e| err("ping conn", e))?;
        conn.simple_query("SELECT 1")
            .await
            .map_err(|e| err("ping", e))?
            .into_results()
            .await
            .map_err(|e| err("ping rows", e))?;
        Ok(())
    }
}

mod ops;
mod schema;
