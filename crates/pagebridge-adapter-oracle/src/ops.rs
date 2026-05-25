//! Oracle statement bodies. All driver calls run inside `pool.with_conn`,
//! which dispatches to a blocking thread.

use oracle::sql_type::ToSql;
use oracle::{Connection, ResultSet, RowValue};

use crate::err;
use crate::pool::OraclePool;
use crate::{level_from_i32, level_to_i32};
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

#[allow(clippy::too_many_lines)]
fn extract_node(row: &oracle::Row) -> Result<NodeRecord> {
    let node_id: String = row.get("node_id").map_err(|e| err("node_id", e))?;
    let doc_id: String = row.get("doc_id").map_err(|e| err("doc_id", e))?;
    let parent_id: Option<String> = row.get("parent_id").map_err(|e| err("parent_id", e))?;
    let title: String = row.get("title").map_err(|e| err("title", e))?;
    let level: i32 = row.get("node_level").map_err(|e| err("node_level", e))?;
    let routing_summary: String = row
        .get("routing_summary")
        .map_err(|e| err("routing_summary", e))?;
    let summary: String = row.get("summary").map_err(|e| err("summary", e))?;
    let child_ids_json: String = row.get("child_ids").map_err(|e| err("child_ids", e))?;
    let span_start: Option<i64> = row.get("span_start").map_err(|e| err("span_start", e))?;
    let span_end: Option<i64> = row.get("span_end").map_err(|e| err("span_end", e))?;
    let page_start: Option<i32> = row.get("page_start").map_err(|e| err("page_start", e))?;
    let page_end: Option<i32> = row.get("page_end").map_err(|e| err("page_end", e))?;
    let keywords_json: String = row.get("keywords").map_err(|e| err("keywords", e))?;
    let is_leaf: i32 = row.get("is_leaf").map_err(|e| err("is_leaf", e))?;
    let created_at: i64 = row.get("created_at").map_err(|e| err("created_at", e))?;
    let updated_at: i64 = row.get("updated_at").map_err(|e| err("updated_at", e))?;
    let source_hash_blob: Vec<u8> = row.get("source_hash").map_err(|e| err("source_hash", e))?;

    let mut hash = [0u8; 32];
    let len = source_hash_blob.len().min(32);
    hash[..len].copy_from_slice(&source_hash_blob[..len]);

    let child_strs: Vec<String> =
        serde_json::from_str(&child_ids_json).map_err(|e| err("decode child_ids", e))?;
    let keywords: Vec<String> =
        serde_json::from_str(&keywords_json).map_err(|e| err("decode keywords", e))?;
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
        is_leaf: is_leaf != 0,
        created_at,
        updated_at,
        source_hash: hash,
    })
}

const NODE_COLS: &str = "node_id, doc_id, parent_id, title, node_level, routing_summary, summary, \
    child_ids, span_start, span_end, page_start, page_end, keywords, is_leaf, created_at, updated_at, source_hash";

pub async fn upsert_node(pool: &OraclePool, node: &NodeRecord) -> Result<()> {
    node.validate()?;
    let child_ids_json = serde_json::to_string(
        &node
            .child_ids
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| err("encode child_ids", e))?;
    let keywords_json = serde_json::to_string(&node.keywords).map_err(|e| err("encode kw", e))?;
    let node_id = node.node_id.as_str().to_owned();
    let doc_id = node.doc_id.as_str().to_owned();
    let parent_id = node.parent_id.as_ref().map(|p| p.as_str().to_owned());
    let title = node.title.clone();
    let level = level_to_i32(node.level);
    let routing_summary = node.routing_summary.clone();
    let summary = node.summary.clone();
    let span_start = node.span.map(|(a, _)| a as i64);
    let span_end = node.span.map(|(_, b)| b as i64);
    let page_start = node.page_start.map(|v| v as i32);
    let page_end = node.page_end.map(|v| v as i32);
    let is_leaf = i32::from(node.is_leaf);
    let created_at = node.created_at;
    let updated_at = node.updated_at;
    let hash = node.source_hash.to_vec();
    pool.with_conn(move |conn| {
        let sql = "MERGE INTO pagebridge_nodes t
            USING (SELECT :1 AS node_id FROM dual) s
            ON (t.node_id = s.node_id)
            WHEN MATCHED THEN UPDATE SET
                doc_id = :2, parent_id = :3, title = :4, node_level = :5,
                routing_summary = :6, summary = :7, child_ids = :8,
                span_start = :9, span_end = :10, page_start = :11, page_end = :12,
                keywords = :13, is_leaf = :14, created_at = :15, updated_at = :16,
                source_hash = :17
            WHEN NOT MATCHED THEN INSERT
                (node_id, doc_id, parent_id, title, node_level, routing_summary, summary, child_ids,
                 span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
                 updated_at, source_hash)
                VALUES (:1, :2, :3, :4, :5, :6, :7, :8,
                        :9, :10, :11, :12, :13, :14, :15, :16, :17)";
        let params: [&dyn ToSql; 17] = [
            &node_id,
            &doc_id,
            &parent_id,
            &title,
            &level,
            &routing_summary,
            &summary,
            &child_ids_json,
            &span_start,
            &span_end,
            &page_start,
            &page_end,
            &keywords_json,
            &is_leaf,
            &created_at,
            &updated_at,
            &hash,
        ];
        conn.execute(sql, &params).map_err(|e| err("upsert", e))?;
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(())
    })
    .await
}

pub async fn get_node(pool: &OraclePool, id: &NodeId) -> Result<Option<NodeRecord>> {
    let id_str = id.as_str().to_owned();
    pool.with_conn(move |conn| {
        let sql = format!("SELECT {NODE_COLS} FROM pagebridge_nodes WHERE node_id = :1");
        let mut rows = conn
            .query_named(&sql, &[("1", &id_str)])
            .map_err(|e| err("get", e))?;
        if let Some(r) = rows.next() {
            let row = r.map_err(|e| err("row", e))?;
            return Ok(Some(extract_node(&row)?));
        }
        Ok(None)
    })
    .await
}

pub async fn children_summaries(pool: &OraclePool, parent: &NodeId) -> Result<Vec<NodeSummary>> {
    let parent_str = parent.as_str().to_owned();
    pool.with_conn(move |conn| {
        let sql = "SELECT node_id, parent_id, title, node_level, routing_summary, is_leaf
                   FROM pagebridge_nodes WHERE parent_id = :1 ORDER BY node_id";
        let rows = conn
            .query(sql, &[&parent_str])
            .map_err(|e| err("children", e))?;
        collect_children_summaries(rows)
    })
    .await
}

fn collect_children_summaries(rows: ResultSet<'_, oracle::Row>) -> Result<Vec<NodeSummary>> {
    let mut out = Vec::new();
    for r in rows {
        let row = r.map_err(|e| err("child row", e))?;
        let node_id: String = row.get(0).map_err(|e| err("child node_id", e))?;
        let parent_id: Option<String> = row.get(1).map_err(|e| err("child parent_id", e))?;
        let title: String = row.get(2).map_err(|e| err("child title", e))?;
        let level: i32 = row.get(3).map_err(|e| err("child level", e))?;
        let routing_summary: String = row.get(4).map_err(|e| err("child rs", e))?;
        let is_leaf: i32 = row.get(5).map_err(|e| err("child is_leaf", e))?;
        out.push(NodeSummary {
            node_id: NodeId::new(node_id)?,
            parent_id: parent_id.map(NodeId::new).transpose()?,
            title,
            level: level_from_i32(level),
            routing_summary,
            is_leaf: is_leaf != 0,
        });
    }
    Ok(out)
}

pub async fn children_records(pool: &OraclePool, parent: &NodeId) -> Result<Vec<NodeRecord>> {
    let parent_str = parent.as_str().to_owned();
    pool.with_conn(move |conn| {
        let sql = format!(
            "SELECT {NODE_COLS} FROM pagebridge_nodes WHERE parent_id = :1 ORDER BY node_id"
        );
        let rows = conn
            .query(&sql, &[&parent_str])
            .map_err(|e| err("crec", e))?;
        let mut out = Vec::new();
        for r in rows {
            let row = r.map_err(|e| err("crec row", e))?;
            out.push(extract_node(&row)?);
        }
        Ok(out)
    })
    .await
}

pub async fn leaves_under(pool: &OraclePool, root: &NodeId) -> Result<Vec<NodeId>> {
    let root_str = root.as_str().to_owned();
    let prefix = format!("{}/%", root.as_str());
    pool.with_conn(move |conn| {
        let sql = "SELECT node_id FROM pagebridge_nodes
                   WHERE (node_id = :1 OR node_id LIKE :2) AND is_leaf = 1
                   ORDER BY node_id";
        let rows = conn
            .query(sql, &[&root_str, &prefix])
            .map_err(|e| err("leaves", e))?;
        let mut out = Vec::new();
        for r in rows {
            let row = r.map_err(|e| err("leaves row", e))?;
            let s: String = row.get(0).map_err(|e| err("leaves col", e))?;
            out.push(NodeId::new(s)?);
        }
        Ok(out)
    })
    .await
}

pub async fn delete_document(pool: &OraclePool, doc_id: &DocId) -> Result<()> {
    let id = doc_id.as_str().to_owned();
    pool.with_conn(move |conn| {
        for sql in [
            "DELETE FROM pagebridge_nodes WHERE doc_id = :1",
            "DELETE FROM pagebridge_raw WHERE doc_id = :1",
            "DELETE FROM pagebridge_docs WHERE doc_id = :1",
        ] {
            conn.execute(sql, &[&id]).map_err(|e| err("delete", e))?;
        }
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(())
    })
    .await
}

pub async fn list_documents(pool: &OraclePool) -> Result<Vec<DocumentEntry>> {
    pool.with_conn(|conn| {
        let sql =
            "SELECT doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count
                   FROM pagebridge_docs ORDER BY doc_id";
        let rows = conn.query(sql, &[]).map_err(|e| err("list", e))?;
        let mut out = Vec::new();
        for r in rows {
            let row = r.map_err(|e| err("list row", e))?;
            let doc_id: String = row.get(0).map_err(|e| err("doc_id", e))?;
            let title: String = row.get(1).map_err(|e| err("title", e))?;
            let source_kind: String = row.get(2).map_err(|e| err("source_kind", e))?;
            let ingested_at: i64 = row.get(3).map_err(|e| err("ingested_at", e))?;
            let root_node_id: String = row.get(4).map_err(|e| err("root", e))?;
            let leaf_count: i32 = row.get(5).map_err(|e| err("leaf_count", e))?;
            let byte_count: i64 = row.get(6).map_err(|e| err("byte_count", e))?;
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
                document_type: None,
            });
        }
        Ok(out)
    })
    .await
}

pub async fn upsert_document(pool: &OraclePool, doc: &DocumentEntry) -> Result<()> {
    let d = doc.doc_id.as_str().to_owned();
    let t = doc.title.clone();
    let k = doc.source_kind.clone();
    let i = doc.ingested_at;
    let r = doc.root_node_id.as_str().to_owned();
    let lc = doc.leaf_count as i32;
    let bc = doc.byte_count as i64;
    pool.with_conn(move |conn| {
        let sql = "MERGE INTO pagebridge_docs t
            USING (SELECT :1 AS doc_id FROM dual) s
            ON (t.doc_id = s.doc_id)
            WHEN MATCHED THEN UPDATE SET
                title = :2, source_kind = :3, ingested_at = :4,
                root_node_id = :5, leaf_count = :6, byte_count = :7
            WHEN NOT MATCHED THEN INSERT
                (doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count)
                VALUES (:1, :2, :3, :4, :5, :6, :7)";
        let params: [&dyn ToSql; 7] = [&d, &t, &k, &i, &r, &lc, &bc];
        conn.execute(sql, &params)
            .map_err(|e| err("upsert doc", e))?;
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(())
    })
    .await
}

pub async fn search(
    pool: &OraclePool,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let q = query.to_owned();
    let like = format!("%{}%", query.replace(['%', '_'], ""));
    let doc_opt = doc.map(|d| d.as_str().to_owned());
    let lim = i32::try_from(limit).unwrap_or(i32::MAX);
    pool.with_conn(move |conn| {
        let _ = q; // reserved for Oracle Text path
        let mut out = Vec::new();
        let rows: Vec<oracle::Row> = if let Some(d) = doc_opt {
            let sql = "SELECT * FROM (
                    SELECT node_id, doc_id, title FROM pagebridge_nodes
                    WHERE is_leaf = 1 AND doc_id = :1
                      AND (title LIKE :2 OR routing_summary LIKE :2 OR DBMS_LOB.INSTR(summary, :3) > 0)
                    ORDER BY LENGTH(title)
                ) WHERE ROWNUM <= :4";
            let params: [&dyn ToSql; 4] = [&d, &like, &like, &lim];
            conn.query(sql, &params)
                .map_err(|e| err("search", e))?
                .filter_map(std::result::Result::ok)
                .collect()
        } else {
            let sql = "SELECT * FROM (
                    SELECT node_id, doc_id, title FROM pagebridge_nodes
                    WHERE is_leaf = 1
                      AND (title LIKE :1 OR routing_summary LIKE :1 OR DBMS_LOB.INSTR(summary, :2) > 0)
                    ORDER BY LENGTH(title)
                ) WHERE ROWNUM <= :3";
            let params: [&dyn ToSql; 3] = [&like, &like, &lim];
            conn.query(sql, &params)
                .map_err(|e| err("search", e))?
                .filter_map(std::result::Result::ok)
                .collect()
        };
        for row in rows {
            let node_id: String = row.get(0).map_err(|e| err("search node_id", e))?;
            let doc_id: String = row.get(1).map_err(|e| err("search doc_id", e))?;
            let title: String = row.get(2).map_err(|e| err("search title", e))?;
            out.push(SearchHit {
                node_id: NodeId::new(node_id)?,
                doc_id: DocId::new(doc_id)?,
                title,
                score: 1.0,
            });
        }
        Ok(out)
    })
    .await
}

pub async fn put_raw(
    pool: &OraclePool,
    doc_id: &DocId,
    data: &[u8],
    chunk_limit: usize,
) -> Result<u64> {
    let id = doc_id.as_str().to_owned();
    let payload = data.to_vec();
    pool.with_conn(move |conn| {
        let start: i64 = conn
            .query_row_as::<i64>(
                "SELECT NVL(MAX(offset_start + length_bytes), 0) FROM pagebridge_raw WHERE doc_id = :1",
                &[&id],
            )
            .map_err(|e| err("raw max", e))?;
        let start_u64 = start as u64;
        let mut written = 0usize;
        while written < payload.len() {
            let take = (payload.len() - written).min(chunk_limit);
            let chunk = &payload[written..written + take];
            let off = (start_u64 + written as u64) as i64;
            let len = chunk.len() as i32;
            let chunk_vec = chunk.to_vec();
            conn.execute(
                "INSERT INTO pagebridge_raw (doc_id, offset_start, length_bytes, data)
                 VALUES (:1, :2, :3, :4)",
                &[&id, &off, &len, &chunk_vec],
            )
            .map_err(|e| err("insert raw", e))?;
            written += take;
        }
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(start_u64)
    })
    .await
}

pub async fn read_raw_span(pool: &OraclePool, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
    if span.0 > span.1 {
        return Err(PagebridgeError::InvalidArgument(format!(
            "span {span:?} start > end"
        )));
    }
    let id = doc_id.as_str().to_owned();
    let a = span.0 as i64;
    let b = span.1 as i64;
    let start = span.0;
    let end = span.1;
    pool.with_conn(move |conn| {
        let rows = conn
            .query(
                "SELECT offset_start, data FROM pagebridge_raw
                 WHERE doc_id = :1 AND offset_start + length_bytes > :2 AND offset_start < :3
                 ORDER BY offset_start",
                &[&id, &a, &b],
            )
            .map_err(|e| err("raw read", e))?;
        let mut out = Vec::with_capacity((end - start) as usize);
        for r in rows {
            let row = r.map_err(|e| err("raw row", e))?;
            let ofs: i64 = row.get(0).map_err(|e| err("raw ofs", e))?;
            let data: Vec<u8> = row.get(1).map_err(|e| err("raw data", e))?;
            let cs = ofs as u64;
            let ce = cs + data.len() as u64;
            let rs = start.max(cs);
            let re = end.min(ce);
            if rs < re {
                let s = (rs - cs) as usize;
                let e = (re - cs) as usize;
                out.extend_from_slice(&data[s..e]);
            }
        }
        if out.len() as u64 != end - start {
            return Err(PagebridgeError::InvalidArgument(format!(
                "short read for ({start}, {end})"
            )));
        }
        Ok(out)
    })
    .await
}

pub async fn get_summary_cache(
    pool: &OraclePool,
    hash: &[u8; 32],
) -> Result<Option<SummaryCacheEntry>> {
    let h = hash.to_vec();
    pool.with_conn(move |conn| {
        let mut rows = conn
            .query(
                "SELECT entry FROM pagebridge_summary_cache WHERE source_hash = :1",
                &[&h],
            )
            .map_err(|e| err("cache get", e))?;
        if let Some(r) = rows.next() {
            let row = r.map_err(|e| err("cache row", e))?;
            let blob: Vec<u8> = row.get(0).map_err(|e| err("cache blob", e))?;
            let entry: SummaryCacheEntry =
                serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
            return Ok(Some(entry));
        }
        Ok(None)
    })
    .await
}

pub async fn upsert_summary_cache(
    pool: &OraclePool,
    hash: &[u8; 32],
    entry: &SummaryCacheEntry,
) -> Result<()> {
    let h = hash.to_vec();
    let blob = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
    pool.with_conn(move |conn| {
        let sql = "MERGE INTO pagebridge_summary_cache t
            USING (SELECT :1 AS source_hash FROM dual) s
            ON (t.source_hash = s.source_hash)
            WHEN MATCHED THEN UPDATE SET entry = :2
            WHEN NOT MATCHED THEN INSERT (source_hash, entry) VALUES (:1, :2)";
        conn.execute(sql, &[&h, &blob])
            .map_err(|e| err("upsert cache", e))?;
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(())
    })
    .await
}

pub async fn stats(pool: &OraclePool) -> Result<AdapterStats> {
    pool.with_conn(|conn| {
        let nodes = count_one(conn, "SELECT COUNT(*) FROM pagebridge_nodes")?;
        let docs = count_one(conn, "SELECT COUNT(*) FROM pagebridge_docs")?;
        let raw = count_one(conn, "SELECT NVL(SUM(length_bytes), 0) FROM pagebridge_raw")?;
        let cache = count_one(conn, "SELECT COUNT(*) FROM pagebridge_summary_cache")?;
        Ok(AdapterStats {
            node_count: nodes as u64,
            document_count: docs as u64,
            raw_bytes: raw as u64,
            summary_cache_entries: cache as u64,
        })
    })
    .await
}

fn count_one(conn: &Connection, sql: &str) -> Result<i64> {
    let n: i64 = conn
        .query_row_as::<i64>(sql, &[])
        .map_err(|e| err(sql, e))?;
    Ok(n)
}

pub async fn ping(pool: &OraclePool) -> Result<()> {
    pool.with_conn(|conn| {
        let _: i64 = conn
            .query_row_as::<i64>("SELECT 1 FROM dual", &[])
            .map_err(|e| err("ping", e))?;
        Ok(())
    })
    .await
}

#[allow(dead_code)]
fn typecheck() {
    fn _row_value<T: RowValue>(_: T) {}
}
