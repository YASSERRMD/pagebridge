//! PostgreSQL storage adapter for pagebridge.
//!
//! Uses sqlx PgPool. BM25 is approximated via Postgres `ts_rank_cd` over a
//! weighted `tsvector` maintained on every node row.

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
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::Row;

const RAW_CHUNK_LIMIT: usize = 1024 * 1024;

/// PostgreSQL storage adapter, backed by sqlx PgPool.
#[derive(Debug, Clone)]
pub struct PostgresAdapter {
    pool: PgPool,
}

impl PostgresAdapter {
    /// Connect to PostgreSQL at the given URL. Pool size 16, acquire timeout 10s.
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(url)
            .await
            .map_err(|e| err("connect", e))?;
        Ok(Self { pool })
    }
}

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "postgres".into(),
        message: format!("{ctx}: {e}"),
    }
}

const fn level_to_i32(l: NodeLevel) -> i32 {
    match l {
        NodeLevel::Corpus => 0,
        NodeLevel::Document => 1,
        NodeLevel::Section => 2,
        NodeLevel::Subsection => 3,
        NodeLevel::Page => 4,
        NodeLevel::Leaf => 5,
    }
}

const fn level_from_i32(v: i32) -> NodeLevel {
    match v {
        0 => NodeLevel::Corpus,
        1 => NodeLevel::Document,
        2 => NodeLevel::Section,
        3 => NodeLevel::Subsection,
        4 => NodeLevel::Page,
        _ => NodeLevel::Leaf,
    }
}

fn row_to_node(row: PgRow) -> Result<NodeRecord> {
    let node_id: String = row.try_get("node_id").map_err(|e| err("col", e))?;
    let doc_id: String = row.try_get("doc_id").map_err(|e| err("col", e))?;
    let parent_id: Option<String> = row.try_get("parent_id").map_err(|e| err("col", e))?;
    let title: String = row.try_get("title").map_err(|e| err("col", e))?;
    let level: i32 = row.try_get("level").map_err(|e| err("col", e))?;
    let routing_summary: String = row.try_get("routing_summary").map_err(|e| err("col", e))?;
    let summary: String = row.try_get("summary").map_err(|e| err("col", e))?;
    let child_ids_json: sqlx::types::JsonValue =
        row.try_get("child_ids").map_err(|e| err("col", e))?;
    let span_start: Option<i64> = row.try_get("span_start").map_err(|e| err("col", e))?;
    let span_end: Option<i64> = row.try_get("span_end").map_err(|e| err("col", e))?;
    let page_start: Option<i32> = row.try_get("page_start").map_err(|e| err("col", e))?;
    let page_end: Option<i32> = row.try_get("page_end").map_err(|e| err("col", e))?;
    let keywords_json: sqlx::types::JsonValue =
        row.try_get("keywords").map_err(|e| err("col", e))?;
    let is_leaf: bool = row.try_get("is_leaf").map_err(|e| err("col", e))?;
    let created_at: i64 = row.try_get("created_at").map_err(|e| err("col", e))?;
    let updated_at: i64 = row.try_get("updated_at").map_err(|e| err("col", e))?;
    let source_hash_blob: Vec<u8> = row.try_get("source_hash").map_err(|e| err("col", e))?;
    let mut hash = [0u8; 32];
    let len = source_hash_blob.len().min(32);
    hash[..len].copy_from_slice(&source_hash_blob[..len]);

    let child_strs: Vec<String> =
        serde_json::from_value(child_ids_json).map_err(|e| err("decode child_ids", e))?;
    let keywords: Vec<String> =
        serde_json::from_value(keywords_json).map_err(|e| err("decode keywords", e))?;
    let mut child_ids = Vec::with_capacity(child_strs.len());
    for c in child_strs {
        child_ids.push(NodeId::new(c)?);
    }

    Ok(NodeRecord {
        node_id: NodeId::new(node_id)?,
        doc_id: DocId::new(doc_id)?,
        parent_id: parent_id.map(NodeId::new).transpose()?,
        title,
        level: level_from_i32(level),
        routing_summary,
        summary,
        child_ids,
        span: match (span_start, span_end) {
            (Some(a), Some(b)) => Some((a as u64, b as u64)),
            _ => None,
        },
        page_start: page_start.map(|v| v as u32),
        page_end: page_end.map(|v| v as u32),
        keywords,
        is_leaf,
        created_at,
        updated_at,
        source_hash: hash,
    })
}

#[async_trait]
impl StorageAdapter for PostgresAdapter {
    fn name(&self) -> &'static str {
        "postgres"
    }

    async fn migrate(&self) -> Result<()> {
        let stmts = [
            "CREATE TABLE IF NOT EXISTS pagebridge_nodes (
                node_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                level INT NOT NULL,
                routing_summary TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                child_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
                span_start BIGINT, span_end BIGINT,
                page_start INT, page_end INT,
                keywords JSONB NOT NULL DEFAULT '[]'::jsonb,
                is_leaf BOOLEAN NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                source_hash BYTEA NOT NULL,
                search_vec tsvector
            )",
            "CREATE INDEX IF NOT EXISTS idx_pb_nodes_doc ON pagebridge_nodes(doc_id)",
            "CREATE INDEX IF NOT EXISTS idx_pb_nodes_parent ON pagebridge_nodes(parent_id)",
            "CREATE INDEX IF NOT EXISTS idx_pb_nodes_fts ON pagebridge_nodes USING GIN(search_vec)",
            "CREATE TABLE IF NOT EXISTS pagebridge_docs (
                doc_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                ingested_at BIGINT NOT NULL,
                root_node_id TEXT NOT NULL,
                leaf_count INT NOT NULL,
                byte_count BIGINT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS pagebridge_raw (
                doc_id TEXT NOT NULL,
                offset_start BIGINT NOT NULL,
                length INT NOT NULL,
                data BYTEA NOT NULL,
                PRIMARY KEY (doc_id, offset_start)
            )",
            "CREATE TABLE IF NOT EXISTS pagebridge_summary_cache (
                source_hash BYTEA PRIMARY KEY,
                entry BYTEA NOT NULL
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
        let child_ids: Vec<String> = node
            .child_ids
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        let child_ids_json = sqlx::types::Json(child_ids);
        let keywords_json = sqlx::types::Json(node.keywords.clone());

        sqlx::query(
            "INSERT INTO pagebridge_nodes (
                node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
                span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
                updated_at, source_hash, search_vec
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                setweight(to_tsvector('english', $4), 'A') ||
                setweight(to_tsvector('english', $6), 'B') ||
                setweight(to_tsvector('english', $7), 'C') ||
                setweight(to_tsvector('english', $18), 'D'))
             ON CONFLICT (node_id) DO UPDATE SET
                doc_id = EXCLUDED.doc_id,
                parent_id = EXCLUDED.parent_id,
                title = EXCLUDED.title,
                level = EXCLUDED.level,
                routing_summary = EXCLUDED.routing_summary,
                summary = EXCLUDED.summary,
                child_ids = EXCLUDED.child_ids,
                span_start = EXCLUDED.span_start,
                span_end = EXCLUDED.span_end,
                page_start = EXCLUDED.page_start,
                page_end = EXCLUDED.page_end,
                keywords = EXCLUDED.keywords,
                is_leaf = EXCLUDED.is_leaf,
                created_at = EXCLUDED.created_at,
                updated_at = EXCLUDED.updated_at,
                source_hash = EXCLUDED.source_hash,
                search_vec = EXCLUDED.search_vec",
        )
        .bind(node.node_id.as_str())
        .bind(node.doc_id.as_str())
        .bind(node.parent_id.as_ref().map(|p| p.as_str().to_owned()))
        .bind(&node.title)
        .bind(level_to_i32(node.level))
        .bind(&node.routing_summary)
        .bind(&node.summary)
        .bind(child_ids_json)
        .bind(node.span.map(|(a, _)| a as i64))
        .bind(node.span.map(|(_, b)| b as i64))
        .bind(node.page_start.map(|v| v as i32))
        .bind(node.page_end.map(|v| v as i32))
        .bind(keywords_json)
        .bind(node.is_leaf)
        .bind(node.created_at)
        .bind(node.updated_at)
        .bind(&node.source_hash[..])
        .bind(node.keywords.join(" "))
        .execute(&self.pool)
        .await
        .map_err(|e| err("upsert_node", e))?;
        Ok(())
    }

    async fn upsert_nodes_batch(&self, nodes: &[NodeRecord]) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }
        for n in nodes {
            n.validate()?;
        }
        let mut tx = self.pool.begin().await.map_err(|e| err("batch tx", e))?;
        for node in nodes {
            let child_ids: Vec<String> = node
                .child_ids
                .iter()
                .map(|c| c.as_str().to_owned())
                .collect();
            let child_ids_json = sqlx::types::Json(child_ids);
            let keywords_json = sqlx::types::Json(node.keywords.clone());
            sqlx::query(
                "INSERT INTO pagebridge_nodes (
                    node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
                    span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
                    updated_at, source_hash, search_vec
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                    setweight(to_tsvector('english', $4), 'A') ||
                    setweight(to_tsvector('english', $6), 'B') ||
                    setweight(to_tsvector('english', $7), 'C') ||
                    setweight(to_tsvector('english', $18), 'D'))
                 ON CONFLICT (node_id) DO UPDATE SET
                    doc_id = EXCLUDED.doc_id,
                    parent_id = EXCLUDED.parent_id,
                    title = EXCLUDED.title,
                    level = EXCLUDED.level,
                    routing_summary = EXCLUDED.routing_summary,
                    summary = EXCLUDED.summary,
                    child_ids = EXCLUDED.child_ids,
                    span_start = EXCLUDED.span_start,
                    span_end = EXCLUDED.span_end,
                    page_start = EXCLUDED.page_start,
                    page_end = EXCLUDED.page_end,
                    keywords = EXCLUDED.keywords,
                    is_leaf = EXCLUDED.is_leaf,
                    created_at = EXCLUDED.created_at,
                    updated_at = EXCLUDED.updated_at,
                    source_hash = EXCLUDED.source_hash,
                    search_vec = EXCLUDED.search_vec",
            )
            .bind(node.node_id.as_str())
            .bind(node.doc_id.as_str())
            .bind(node.parent_id.as_ref().map(|p| p.as_str().to_owned()))
            .bind(&node.title)
            .bind(level_to_i32(node.level))
            .bind(&node.routing_summary)
            .bind(&node.summary)
            .bind(child_ids_json)
            .bind(node.span.map(|(a, _)| a as i64))
            .bind(node.span.map(|(_, b)| b as i64))
            .bind(node.page_start.map(|v| v as i32))
            .bind(node.page_end.map(|v| v as i32))
            .bind(keywords_json)
            .bind(node.is_leaf)
            .bind(node.created_at)
            .bind(node.updated_at)
            .bind(&node.source_hash[..])
            .bind(node.keywords.join(" "))
            .execute(&mut *tx)
            .await
            .map_err(|e| err("batch upsert_node", e))?;
        }
        tx.commit().await.map_err(|e| err("batch commit", e))?;
        Ok(())
    }

    fn recommended_batch_size(&self) -> usize {
        1_000
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let row = sqlx::query("SELECT * FROM pagebridge_nodes WHERE node_id = $1")
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
             FROM pagebridge_nodes WHERE parent_id = $1 ORDER BY node_id",
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
            let level: i32 = row.try_get("level").map_err(|e| err("col", e))?;
            let rs: String = row.try_get("routing_summary").map_err(|e| err("col", e))?;
            let is_leaf: bool = row.try_get("is_leaf").map_err(|e| err("col", e))?;
            out.push(NodeSummary {
                node_id: NodeId::new(node_id)?,
                parent_id: parent_id.map(NodeId::new).transpose()?,
                title,
                level: level_from_i32(level),
                routing_summary: rs,
                is_leaf,
            });
        }
        Ok(out)
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        let rows =
            sqlx::query("SELECT * FROM pagebridge_nodes WHERE parent_id = $1 ORDER BY node_id")
                .bind(parent.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err("children_records", e))?;
        rows.into_iter().map(row_to_node).collect()
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
             WHERE (node_id = $1 OR node_id LIKE $2) AND is_leaf = TRUE
             ORDER BY node_id",
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
        sqlx::query("DELETE FROM pagebridge_nodes WHERE doc_id = $1")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("delete nodes", e))?;
        sqlx::query("DELETE FROM pagebridge_raw WHERE doc_id = $1")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("delete raw", e))?;
        sqlx::query("DELETE FROM pagebridge_docs WHERE doc_id = $1")
            .bind(doc_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| err("delete docs", e))?;
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
            let leaf_count: i32 = row.try_get("leaf_count").map_err(|e| err("col", e))?;
            let byte_count: i64 = row.try_get("byte_count").map_err(|e| err("col", e))?;
            out.push(DocumentEntry {
                doc_id: DocId::new(doc_id)?,
                title,
                source_kind,
                ingested_at,
                root_node_id: NodeId::new(root_node_id)?,
                leaf_count: leaf_count as u32,
                byte_count: byte_count as u64,
                raw_text_hash: None,
                structural_hash: None,
            });
        }
        Ok(out)
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO pagebridge_docs
             (doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count)
             VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (doc_id) DO UPDATE SET
               title = EXCLUDED.title,
               source_kind = EXCLUDED.source_kind,
               ingested_at = EXCLUDED.ingested_at,
               root_node_id = EXCLUDED.root_node_id,
               leaf_count = EXCLUDED.leaf_count,
               byte_count = EXCLUDED.byte_count",
        )
        .bind(doc.doc_id.as_str())
        .bind(&doc.title)
        .bind(&doc.source_kind)
        .bind(doc.ingested_at)
        .bind(doc.root_node_id.as_str())
        .bind(doc.leaf_count as i32)
        .bind(doc.byte_count as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| err("upsert_document", e))?;
        Ok(())
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        search_impl(&self.pool, query, limit, None).await
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        search_impl(&self.pool, query, limit, Some(doc_id)).await
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(offset_start + length), 0)::BIGINT AS end_off
             FROM pagebridge_raw WHERE doc_id = $1",
        )
        .bind(doc_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| err("max offset", e))?;
        let start: i64 = row.try_get("end_off").map_err(|e| err("col", e))?;
        let start_u64 = start as u64;
        let mut written = 0usize;
        while written < data.len() {
            let take = (data.len() - written).min(RAW_CHUNK_LIMIT);
            let chunk = &data[written..written + take];
            let chunk_off = start_u64 + written as u64;
            sqlx::query(
                "INSERT INTO pagebridge_raw (doc_id, offset_start, length, data)
                 VALUES ($1,$2,$3,$4)",
            )
            .bind(doc_id.as_str())
            .bind(chunk_off as i64)
            .bind(chunk.len() as i32)
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
             WHERE doc_id = $1 AND offset_start + length > $2 AND offset_start < $3
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
            let cs = ofs as u64;
            let ce = cs + data.len() as u64;
            let rs = span.0.max(cs);
            let re = span.1.min(ce);
            if rs < re {
                let s = (rs - cs) as usize;
                let e = (re - cs) as usize;
                out.extend_from_slice(&data[s..e]);
            }
        }
        if out.len() as u64 != span.1 - span.0 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "short read for {span:?}"
            )));
        }
        Ok(out)
    }

    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        let row = sqlx::query("SELECT entry FROM pagebridge_summary_cache WHERE source_hash = $1")
            .bind(&hash[..])
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| err("get_summary_cache", e))?;
        let Some(row) = row else { return Ok(None) };
        let blob: Vec<u8> = row.try_get("entry").map_err(|e| err("col", e))?;
        let entry: SummaryCacheEntry =
            serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
        Ok(Some(entry))
    }

    async fn get_summary_caches_batch(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<std::collections::HashMap<[u8; 32], SummaryCacheEntry>> {
        if hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        // Postgres takes a BYTEA[] easily via ANY($1).
        let owned: Vec<Vec<u8>> = hashes.iter().map(|h| h.to_vec()).collect();
        let rows = sqlx::query(
            "SELECT source_hash, entry FROM pagebridge_summary_cache WHERE source_hash = ANY($1)",
        )
        .bind(&owned)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("batch get_summary_cache", e))?;
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let hbytes: Vec<u8> = row.try_get("source_hash").map_err(|e| err("col hash", e))?;
            let mut harr = [0u8; 32];
            if hbytes.len() == 32 {
                harr.copy_from_slice(&hbytes);
            }
            let blob: Vec<u8> = row.try_get("entry").map_err(|e| err("col entry", e))?;
            let entry: SummaryCacheEntry =
                serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
            out.insert(harr, entry);
        }
        Ok(out)
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        let blob = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
        sqlx::query(
            "INSERT INTO pagebridge_summary_cache (source_hash, entry) VALUES ($1,$2)
             ON CONFLICT (source_hash) DO UPDATE SET entry = EXCLUDED.entry",
        )
        .bind(&hash[..])
        .bind(blob)
        .execute(&self.pool)
        .await
        .map_err(|e| err("upsert cache", e))?;
        Ok(())
    }

    async fn stats(&self) -> Result<AdapterStats> {
        let row = sqlx::query(
            "SELECT
                (SELECT COUNT(*) FROM pagebridge_nodes)::BIGINT AS nodes,
                (SELECT COUNT(*) FROM pagebridge_docs)::BIGINT AS docs,
                (SELECT COALESCE(SUM(length), 0)::BIGINT FROM pagebridge_raw) AS raw,
                (SELECT COUNT(*) FROM pagebridge_summary_cache)::BIGINT AS cache",
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

async fn search_impl(
    pool: &PgPool,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let sql = if doc.is_some() {
        "SELECT node_id, doc_id, title,
                ts_rank_cd(search_vec, q) AS score
         FROM pagebridge_nodes, plainto_tsquery('english', $1) AS q
         WHERE search_vec @@ q AND is_leaf = TRUE AND doc_id = $2
         ORDER BY score DESC LIMIT $3"
    } else {
        "SELECT node_id, doc_id, title,
                ts_rank_cd(search_vec, q) AS score
         FROM pagebridge_nodes, plainto_tsquery('english', $1) AS q
         WHERE search_vec @@ q AND is_leaf = TRUE
         ORDER BY score DESC LIMIT $2"
    };
    let mut q = sqlx::query(sql).bind(query);
    if let Some(d) = doc {
        q = q.bind(d.as_str()).bind(limit as i64);
    } else {
        q = q.bind(limit as i64);
    }
    let rows = q.fetch_all(pool).await.map_err(|e| err("search", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let node_id: String = row.try_get("node_id").map_err(|e| err("col", e))?;
        let doc_id: String = row.try_get("doc_id").map_err(|e| err("col", e))?;
        let title: String = row.try_get("title").map_err(|e| err("col", e))?;
        let score: f32 = row.try_get("score").map_err(|e| err("col", e))?;
        out.push(SearchHit {
            node_id: NodeId::new(node_id)?,
            doc_id: DocId::new(doc_id)?,
            title,
            score,
        });
    }
    Ok(out)
}
