//! SQLite storage adapter for pagebridge.
//!
//! Single-file SQLite database with WAL mode and FTS5 for BM25.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]

use std::path::Path;
use std::str::FromStr;

use async_trait::async_trait;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

const RAW_CHUNK_LIMIT: usize = 256 * 1024;

/// SQLite storage adapter, backed by a single file with FTS5 BM25.
#[derive(Debug, Clone)]
pub struct SqliteAdapter {
    pool: SqlitePool,
}

impl SqliteAdapter {
    /// Open or create a SQLite database at `path` with WAL mode.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path.as_ref())
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(16)
            .connect_with(opts)
            .await
            .map_err(|e| err("connect", e))?;
        Ok(Self { pool })
    }

    /// Open an in-memory SQLite for tests.
    pub async fn memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| err("parse opts", e))?
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(|e| err("memory connect", e))?;
        Ok(Self { pool })
    }
}

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "sqlite".into(),
        message: format!("{ctx}: {e}"),
    }
}

const fn level_to_i64(l: NodeLevel) -> i64 {
    match l {
        NodeLevel::Corpus => 0,
        NodeLevel::Document => 1,
        NodeLevel::Section => 2,
        NodeLevel::Subsection => 3,
        NodeLevel::Page => 4,
        NodeLevel::Leaf => 5,
    }
}

const fn level_from_i64(v: i64) -> NodeLevel {
    match v {
        0 => NodeLevel::Corpus,
        1 => NodeLevel::Document,
        2 => NodeLevel::Section,
        3 => NodeLevel::Subsection,
        4 => NodeLevel::Page,
        _ => NodeLevel::Leaf,
    }
}

fn row_to_node(row: sqlx::sqlite::SqliteRow) -> Result<NodeRecord> {
    let node_id: String = row.try_get("node_id").map_err(|e| err("col node_id", e))?;
    let doc_id: String = row.try_get("doc_id").map_err(|e| err("col doc_id", e))?;
    let parent_id: Option<String> = row
        .try_get("parent_id")
        .map_err(|e| err("col parent_id", e))?;
    let title: String = row.try_get("title").map_err(|e| err("col title", e))?;
    let level: i64 = row.try_get("level").map_err(|e| err("col level", e))?;
    let routing_summary: String = row
        .try_get("routing_summary")
        .map_err(|e| err("col rs", e))?;
    let summary: String = row.try_get("summary").map_err(|e| err("col summary", e))?;
    let child_ids: String = row
        .try_get("child_ids")
        .map_err(|e| err("col child_ids", e))?;
    let span_start: Option<i64> = row.try_get("span_start").map_err(|e| err("col ss", e))?;
    let span_end: Option<i64> = row.try_get("span_end").map_err(|e| err("col se", e))?;
    let page_start: Option<i64> = row.try_get("page_start").map_err(|e| err("col ps", e))?;
    let page_end: Option<i64> = row.try_get("page_end").map_err(|e| err("col pe", e))?;
    let keywords: String = row.try_get("keywords").map_err(|e| err("col kw", e))?;
    let is_leaf: i64 = row.try_get("is_leaf").map_err(|e| err("col is_leaf", e))?;
    let created_at: i64 = row
        .try_get("created_at")
        .map_err(|e| err("col created", e))?;
    let updated_at: i64 = row
        .try_get("updated_at")
        .map_err(|e| err("col updated", e))?;
    let source_hash_blob: Vec<u8> = row
        .try_get("source_hash")
        .map_err(|e| err("col source_hash", e))?;
    let mut hash = [0u8; 32];
    let len = source_hash_blob.len().min(32);
    hash[..len].copy_from_slice(&source_hash_blob[..len]);
    let child_ids: Vec<String> =
        serde_json::from_str(&child_ids).map_err(|e| err("decode child_ids", e))?;
    let keywords: Vec<String> =
        serde_json::from_str(&keywords).map_err(|e| err("decode keywords", e))?;
    let mut child_node_ids = Vec::with_capacity(child_ids.len());
    for c in child_ids {
        child_node_ids.push(NodeId::new(c)?);
    }
    Ok(NodeRecord {
        node_id: NodeId::new(node_id)?,
        doc_id: DocId::new(doc_id)?,
        parent_id: parent_id.map(NodeId::new).transpose()?,
        title,
        level: level_from_i64(level),
        routing_summary,
        summary,
        child_ids: child_node_ids,
        span: match (span_start, span_end) {
            (Some(a), Some(b)) => Some((a as u64, b as u64)),
            _ => None,
        },
        page_start: page_start.map(|v| v as u32),
        page_end: page_end.map(|v| v as u32),
        keywords,
        is_leaf: is_leaf != 0,
        created_at,
        updated_at,
        source_hash: hash,
    })
}

#[async_trait]
impl StorageAdapter for SqliteAdapter {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    async fn migrate(&self) -> Result<()> {
        let stmts = [
            "CREATE TABLE IF NOT EXISTS pagebridge_nodes (
                node_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                level INTEGER NOT NULL,
                routing_summary TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                child_ids TEXT NOT NULL DEFAULT '[]',
                span_start INTEGER, span_end INTEGER,
                page_start INTEGER, page_end INTEGER,
                keywords TEXT NOT NULL DEFAULT '[]',
                is_leaf INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source_hash BLOB NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_nodes_doc ON pagebridge_nodes(doc_id)",
            "CREATE INDEX IF NOT EXISTS idx_nodes_parent ON pagebridge_nodes(parent_id)",
            "CREATE VIRTUAL TABLE IF NOT EXISTS pagebridge_fts USING fts5(
                node_id UNINDEXED, doc_id UNINDEXED, title, routing_summary, summary, keywords
            )",
            "CREATE TABLE IF NOT EXISTS pagebridge_docs (
                doc_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                ingested_at INTEGER NOT NULL,
                root_node_id TEXT NOT NULL,
                leaf_count INTEGER NOT NULL,
                byte_count INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS pagebridge_raw (
                doc_id TEXT NOT NULL,
                offset_start INTEGER NOT NULL,
                length INTEGER NOT NULL,
                data BLOB NOT NULL,
                PRIMARY KEY (doc_id, offset_start)
            )",
            "CREATE TABLE IF NOT EXISTS pagebridge_summary_cache (
                source_hash BLOB PRIMARY KEY,
                entry BLOB NOT NULL
            )",
        ];
        for s in stmts {
            sqlx::query(s)
                .execute(&self.pool)
                .await
                .map_err(|e| err("migrate", e))?;
        }
        Ok(())
    }

    async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
        node.validate()?;
        let mut tx = self.pool.begin().await.map_err(|e| err("tx", e))?;
        let child_ids_json: Vec<String> = node
            .child_ids
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        let child_ids =
            serde_json::to_string(&child_ids_json).map_err(|e| err("encode kids", e))?;
        let keywords = serde_json::to_string(&node.keywords).map_err(|e| err("encode kw", e))?;
        sqlx::query(
            "INSERT INTO pagebridge_nodes (
                node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
                span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
                updated_at, source_hash
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(node_id) DO UPDATE SET
                doc_id=excluded.doc_id, parent_id=excluded.parent_id, title=excluded.title,
                level=excluded.level, routing_summary=excluded.routing_summary,
                summary=excluded.summary, child_ids=excluded.child_ids,
                span_start=excluded.span_start, span_end=excluded.span_end,
                page_start=excluded.page_start, page_end=excluded.page_end,
                keywords=excluded.keywords, is_leaf=excluded.is_leaf,
                created_at=excluded.created_at, updated_at=excluded.updated_at,
                source_hash=excluded.source_hash",
        )
        .bind(node.node_id.as_str())
        .bind(node.doc_id.as_str())
        .bind(node.parent_id.as_ref().map(|p| p.as_str().to_owned()))
        .bind(&node.title)
        .bind(level_to_i64(node.level))
        .bind(&node.routing_summary)
        .bind(&node.summary)
        .bind(child_ids)
        .bind(node.span.map(|(a, _)| a as i64))
        .bind(node.span.map(|(_, b)| b as i64))
        .bind(node.page_start.map(i64::from))
        .bind(node.page_end.map(i64::from))
        .bind(keywords)
        .bind(i64::from(node.is_leaf))
        .bind(node.created_at)
        .bind(node.updated_at)
        .bind(&node.source_hash[..])
        .execute(&mut *tx)
        .await
        .map_err(|e| err("upsert node", e))?;

        // FTS5 contentless table: delete + insert.
        sqlx::query("DELETE FROM pagebridge_fts WHERE node_id = ?")
            .bind(node.node_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("fts delete", e))?;
        let kw_text = node.keywords.join(" ");
        sqlx::query(
            "INSERT INTO pagebridge_fts (node_id, doc_id, title, routing_summary, summary, keywords)
             VALUES (?,?,?,?,?,?)",
        )
        .bind(node.node_id.as_str())
        .bind(node.doc_id.as_str())
        .bind(&node.title)
        .bind(&node.routing_summary)
        .bind(&node.summary)
        .bind(kw_text)
        .execute(&mut *tx)
        .await
        .map_err(|e| err("fts insert", e))?;

        tx.commit().await.map_err(|e| err("commit", e))?;
        Ok(())
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let row = sqlx::query("SELECT * FROM pagebridge_nodes WHERE node_id = ?")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| err("get_node", e))?;
        row.map(row_to_node).transpose()
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
        let rows = sqlx::query(
            "SELECT node_id, parent_id, title, level, routing_summary, is_leaf
             FROM pagebridge_nodes WHERE parent_id = ? ORDER BY node_id",
        )
        .bind(parent.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("children_summaries", e))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let node_id: String = row.try_get("node_id").map_err(|e| err("col", e))?;
            let parent_id: Option<String> = row.try_get("parent_id").map_err(|e| err("col", e))?;
            let title: String = row.try_get("title").map_err(|e| err("col", e))?;
            let level: i64 = row.try_get("level").map_err(|e| err("col", e))?;
            let rs: String = row.try_get("routing_summary").map_err(|e| err("col", e))?;
            let is_leaf: i64 = row.try_get("is_leaf").map_err(|e| err("col", e))?;
            out.push(NodeSummary {
                node_id: NodeId::new(node_id)?,
                parent_id: parent_id.map(NodeId::new).transpose()?,
                title,
                level: level_from_i64(level),
                routing_summary: rs,
                is_leaf: is_leaf != 0,
            });
        }
        Ok(out)
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        let rows =
            sqlx::query("SELECT * FROM pagebridge_nodes WHERE parent_id = ? ORDER BY node_id")
                .bind(parent.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err("children_records", e))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(row_to_node(row)?);
        }
        Ok(out)
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
        let prefix = format!("{}/%", root.as_str());
        let rows = sqlx::query(
            "SELECT node_id FROM pagebridge_nodes
             WHERE (node_id = ? OR node_id LIKE ?) AND is_leaf = 1 ORDER BY node_id",
        )
        .bind(root.as_str())
        .bind(prefix)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("leaves_under", e))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("node_id").map_err(|e| err("col", e))?;
            out.push(NodeId::new(id)?);
        }
        Ok(out)
    }

    async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| err("tx", e))?;
        sqlx::query("DELETE FROM pagebridge_fts WHERE doc_id = ?")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("fts delete", e))?;
        sqlx::query("DELETE FROM pagebridge_nodes WHERE doc_id = ?")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("nodes delete", e))?;
        sqlx::query("DELETE FROM pagebridge_raw WHERE doc_id = ?")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("raw delete", e))?;
        sqlx::query("DELETE FROM pagebridge_docs WHERE doc_id = ?")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("doc delete", e))?;
        tx.commit().await.map_err(|e| err("commit", e))?;
        Ok(())
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        let rows = sqlx::query("SELECT * FROM pagebridge_docs ORDER BY doc_id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err("list_documents", e))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let doc_id: String = row.try_get("doc_id").map_err(|e| err("col", e))?;
            let title: String = row.try_get("title").map_err(|e| err("col", e))?;
            let source_kind: String = row.try_get("source_kind").map_err(|e| err("col", e))?;
            let ingested_at: i64 = row.try_get("ingested_at").map_err(|e| err("col", e))?;
            let root_node_id: String = row.try_get("root_node_id").map_err(|e| err("col", e))?;
            let leaf_count: i64 = row.try_get("leaf_count").map_err(|e| err("col", e))?;
            let byte_count: i64 = row.try_get("byte_count").map_err(|e| err("col", e))?;
            out.push(DocumentEntry {
                doc_id: DocId::new(doc_id)?,
                title,
                source_kind,
                ingested_at,
                root_node_id: NodeId::new(root_node_id)?,
                leaf_count: leaf_count as u32,
                byte_count: byte_count as u64,
            });
        }
        Ok(out)
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO pagebridge_docs
             (doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count)
             VALUES (?,?,?,?,?,?,?)
             ON CONFLICT(doc_id) DO UPDATE SET
               title=excluded.title, source_kind=excluded.source_kind,
               ingested_at=excluded.ingested_at, root_node_id=excluded.root_node_id,
               leaf_count=excluded.leaf_count, byte_count=excluded.byte_count",
        )
        .bind(doc.doc_id.as_str())
        .bind(&doc.title)
        .bind(&doc.source_kind)
        .bind(doc.ingested_at)
        .bind(doc.root_node_id.as_str())
        .bind(i64::from(doc.leaf_count))
        .bind(doc.byte_count as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| err("upsert_document", e))?;
        Ok(())
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        bm25_search_impl(&self.pool, query, limit, None).await
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        bm25_search_impl(&self.pool, query, limit, Some(doc_id)).await
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(offset_start + length), 0) AS end FROM pagebridge_raw WHERE doc_id = ?",
        )
        .bind(doc_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| err("max offset", e))?;
        let start: i64 = row.try_get("end").map_err(|e| err("col end", e))?;
        let start_u64 = start as u64;
        let mut written = 0usize;
        while written < data.len() {
            let take = (data.len() - written).min(RAW_CHUNK_LIMIT);
            let chunk = &data[written..written + take];
            let chunk_off = start_u64 + written as u64;
            sqlx::query(
                "INSERT INTO pagebridge_raw (doc_id, offset_start, length, data) VALUES (?,?,?,?)",
            )
            .bind(doc_id.as_str())
            .bind(chunk_off as i64)
            .bind(chunk.len() as i64)
            .bind(chunk)
            .execute(&self.pool)
            .await
            .map_err(|e| err("insert raw", e))?;
            written += take;
        }
        Ok(start_u64)
    }

    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
        if span.0 > span.1 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "span {span:?} start > end"
            )));
        }
        let rows = sqlx::query(
            "SELECT offset_start, length, data FROM pagebridge_raw
             WHERE doc_id = ? AND offset_start + length > ? AND offset_start < ?
             ORDER BY offset_start",
        )
        .bind(doc_id.as_str())
        .bind(span.0 as i64)
        .bind(span.1 as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("read raw", e))?;
        let mut out = Vec::with_capacity((span.1 - span.0) as usize);
        for row in rows {
            let ofs: i64 = row.try_get("offset_start").map_err(|e| err("col", e))?;
            let data: Vec<u8> = row.try_get("data").map_err(|e| err("col", e))?;
            let chunk_start = ofs as u64;
            let chunk_end = chunk_start + data.len() as u64;
            let read_start = span.0.max(chunk_start);
            let read_end = span.1.min(chunk_end);
            if read_start < read_end {
                let s = (read_start - chunk_start) as usize;
                let e = (read_end - chunk_start) as usize;
                out.extend_from_slice(&data[s..e]);
            }
        }
        if out.len() as u64 != span.1 - span.0 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "read_raw_span: short read for {span:?}, got {} bytes",
                out.len()
            )));
        }
        Ok(out)
    }

    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        let row = sqlx::query("SELECT entry FROM pagebridge_summary_cache WHERE source_hash = ?")
            .bind(&hash[..])
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| err("get_summary_cache", e))?;
        let Some(row) = row else { return Ok(None) };
        let blob: Vec<u8> = row.try_get("entry").map_err(|e| err("col entry", e))?;
        let entry: SummaryCacheEntry =
            serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
        Ok(Some(entry))
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        let blob = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
        sqlx::query(
            "INSERT INTO pagebridge_summary_cache (source_hash, entry) VALUES (?,?)
             ON CONFLICT(source_hash) DO UPDATE SET entry=excluded.entry",
        )
        .bind(&hash[..])
        .bind(blob)
        .execute(&self.pool)
        .await
        .map_err(|e| err("upsert_summary_cache", e))?;
        Ok(())
    }

    async fn stats(&self) -> Result<AdapterStats> {
        let row = sqlx::query(
            "SELECT
              (SELECT COUNT(*) FROM pagebridge_nodes) AS nodes,
              (SELECT COUNT(*) FROM pagebridge_docs) AS docs,
              (SELECT COALESCE(SUM(length), 0) FROM pagebridge_raw) AS raw,
              (SELECT COUNT(*) FROM pagebridge_summary_cache) AS cache",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| err("stats", e))?;
        let n: i64 = row.try_get("nodes").map_err(|e| err("col", e))?;
        let d: i64 = row.try_get("docs").map_err(|e| err("col", e))?;
        let r: i64 = row.try_get("raw").map_err(|e| err("col", e))?;
        let c: i64 = row.try_get("cache").map_err(|e| err("col", e))?;
        Ok(AdapterStats {
            node_count: n as u64,
            document_count: d as u64,
            raw_bytes: r as u64,
            summary_cache_entries: c as u64,
        })
    }

    async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| err("ping", e))?;
        Ok(())
    }
}

async fn bm25_search_impl(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Strip characters FTS5 might interpret as operators when used naively.
    let safe_query = query
        .chars()
        .map(|c| if matches!(c, '"' | '\\') { ' ' } else { c })
        .collect::<String>();
    let sql = if doc.is_some() {
        "SELECT pagebridge_fts.node_id, pagebridge_fts.doc_id, pagebridge_fts.title,
                bm25(pagebridge_fts) AS score
         FROM pagebridge_fts
         WHERE pagebridge_fts MATCH ? AND doc_id = ?
         ORDER BY score
         LIMIT ?"
    } else {
        "SELECT pagebridge_fts.node_id, pagebridge_fts.doc_id, pagebridge_fts.title,
                bm25(pagebridge_fts) AS score
         FROM pagebridge_fts
         WHERE pagebridge_fts MATCH ?
         ORDER BY score
         LIMIT ?"
    };
    let mut q = sqlx::query(sql).bind(&safe_query);
    if let Some(d) = doc {
        q = q.bind(d.as_str());
    }
    let rows = q
        .bind(limit as i64)
        .fetch_all(pool)
        .await
        .map_err(|e| err("fts search", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let node_id: String = row.try_get("node_id").map_err(|e| err("col", e))?;
        let doc_id: String = row.try_get("doc_id").map_err(|e| err("col", e))?;
        let title: String = row.try_get("title").map_err(|e| err("col", e))?;
        let raw_score: f64 = row.try_get("score").map_err(|e| err("col", e))?;
        out.push(SearchHit {
            node_id: NodeId::new(node_id)?,
            doc_id: DocId::new(doc_id)?,
            title,
            // SQLite FTS5 bm25 returns negative numbers (smaller = better).
            // Convert to "higher is better".
            score: (-raw_score) as f32,
        });
    }
    Ok(out)
}
