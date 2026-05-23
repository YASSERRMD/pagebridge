//! MySQL / MariaDB storage adapter for pagebridge.
//!
//! Uses `mysql_async::Pool`. Full-text search is approximated via
//! `MATCH ... AGAINST` over the (title, routing_summary, summary) columns.
//! Scores are normalized so "higher is more relevant" matches the convention
//! used by every other adapter.

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
use mysql_async::prelude::*;
use mysql_async::{Pool, PoolOpts};
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

const RAW_CHUNK_LIMIT: usize = 8 * 1024 * 1024;

/// MySQL/MariaDB storage adapter, backed by `mysql_async::Pool`.
#[derive(Debug, Clone)]
pub struct MySqlAdapter {
    pool: Pool,
}

impl MySqlAdapter {
    /// Connect to MySQL/MariaDB at the given URL. Pool capped at 16 concurrent
    /// connections, idle connections released after 60 seconds.
    pub async fn connect(url: &str) -> Result<Self> {
        let opts = mysql_async::OptsBuilder::from_opts(
            mysql_async::Opts::from_url(url).map_err(|e| err("parse url", e))?,
        )
        .pool_opts(PoolOpts::default().with_constraints(
            mysql_async::PoolConstraints::new(0, 16).unwrap_or_default(),
        ));
        let pool = Pool::new(opts);
        let adapter = Self { pool };
        adapter.ping().await?;
        Ok(adapter)
    }
}

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "mysql".into(),
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
impl StorageAdapter for MySqlAdapter {
    fn name(&self) -> &'static str {
        "mysql"
    }

    async fn migrate(&self) -> Result<()> {
        // Schema lives in the schema module so the upsert/query paths can stay tidy.
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

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        crate::ops::upsert_summary_cache(&self.pool, hash, entry).await
    }

    async fn stats(&self) -> Result<AdapterStats> {
        crate::ops::stats(&self.pool).await
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self.pool.get_conn().await.map_err(|e| err("ping", e))?;
        "SELECT 1"
            .ignore(&mut conn)
            .await
            .map_err(|e| err("ping", e))?;
        Ok(())
    }
}

mod schema;
mod ops;
