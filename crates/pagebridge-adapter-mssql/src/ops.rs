//! Statement bodies for the SQL Server adapter.

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use tiberius::{Query, Row};

use crate::{err, level_from_i32, level_to_i32, MsPool};

fn opt_str(row: &Row, idx: usize, label: &str) -> Result<Option<String>> {
    row.try_get::<&str, _>(idx)
        .map(|o| o.map(str::to_owned))
        .map_err(|e| err(label, e))
}

fn req_str(row: &Row, idx: usize, label: &str) -> Result<String> {
    opt_str(row, idx, label)?.ok_or_else(|| err("null", label))
}

fn opt_i64(row: &Row, idx: usize, label: &str) -> Result<Option<i64>> {
    row.try_get::<i64, _>(idx).map_err(|e| err(label, e))
}

fn req_i64(row: &Row, idx: usize, label: &str) -> Result<i64> {
    opt_i64(row, idx, label)?.ok_or_else(|| err("null", label))
}

fn opt_i32(row: &Row, idx: usize, label: &str) -> Result<Option<i32>> {
    row.try_get::<i32, _>(idx).map_err(|e| err(label, e))
}

fn req_i32(row: &Row, idx: usize, label: &str) -> Result<i32> {
    opt_i32(row, idx, label)?.ok_or_else(|| err("null", label))
}

fn req_u8(row: &Row, idx: usize, label: &str) -> Result<u8> {
    row.try_get::<u8, _>(idx)
        .map_err(|e| err(label, e))?
        .ok_or_else(|| err("null", label))
}

fn req_bool(row: &Row, idx: usize, label: &str) -> Result<bool> {
    row.try_get::<bool, _>(idx)
        .map_err(|e| err(label, e))?
        .ok_or_else(|| err("null", label))
}

fn opt_bytes(row: &Row, idx: usize, label: &str) -> Result<Option<Vec<u8>>> {
    row.try_get::<&[u8], _>(idx)
        .map(|o| o.map(<[u8]>::to_vec))
        .map_err(|e| err(label, e))
}

fn req_bytes(row: &Row, idx: usize, label: &str) -> Result<Vec<u8>> {
    opt_bytes(row, idx, label)?.ok_or_else(|| err("null", label))
}

fn row_to_node(row: &Row) -> Result<NodeRecord> {
    let node_id = req_str(row, 0, "node_id")?;
    let doc_id = req_str(row, 1, "doc_id")?;
    let parent_id = opt_str(row, 2, "parent_id")?;
    let title = req_str(row, 3, "title")?;
    let level = req_u8(row, 4, "level")?;
    let routing_summary = req_str(row, 5, "routing_summary")?;
    let summary = req_str(row, 6, "summary")?;
    let child_ids_json = req_str(row, 7, "child_ids")?;
    let span_start = opt_i64(row, 8, "span_start")?;
    let span_end = opt_i64(row, 9, "span_end")?;
    let page_start = opt_i32(row, 10, "page_start")?;
    let page_end = opt_i32(row, 11, "page_end")?;
    let keywords_json = req_str(row, 12, "keywords")?;
    let is_leaf = req_bool(row, 13, "is_leaf")?;
    let created_at = req_i64(row, 14, "created_at")?;
    let updated_at = req_i64(row, 15, "updated_at")?;
    let source_hash_blob = req_bytes(row, 16, "source_hash")?;

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
        level: level_from_i32(i32::from(level)),
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

const NODE_COLS: &str =
    "node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids, \
     span_start, span_end, page_start, page_end, keywords, is_leaf, created_at, updated_at, source_hash";

pub async fn upsert_node(pool: &MsPool, node: &NodeRecord) -> Result<()> {
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

    let mut conn = pool.get().await.map_err(|e| err("upsert conn", e))?;
    let sql = "MERGE INTO pagebridge_nodes AS target
        USING (SELECT @P1 AS node_id) AS src
        ON target.node_id = src.node_id
        WHEN MATCHED THEN UPDATE SET
            doc_id = @P2, parent_id = @P3, title = @P4, level = @P5,
            routing_summary = @P6, summary = @P7, child_ids = @P8,
            span_start = @P9, span_end = @P10, page_start = @P11, page_end = @P12,
            keywords = @P13, is_leaf = @P14, created_at = @P15, updated_at = @P16,
            source_hash = @P17
        WHEN NOT MATCHED THEN INSERT
            (node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
             span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
             updated_at, source_hash)
        VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8,
                @P9, @P10, @P11, @P12, @P13, @P14, @P15, @P16, @P17);";
    let mut q = Query::new(sql);
    q.bind(node.node_id.as_str());
    q.bind(node.doc_id.as_str());
    match node.parent_id.as_ref() {
        Some(p) => q.bind(p.as_str()),
        None => q.bind(Option::<String>::None),
    }
    q.bind(node.title.as_str());
    q.bind(u8::try_from(level_to_i32(node.level)).unwrap_or(0));
    q.bind(node.routing_summary.as_str());
    q.bind(node.summary.as_str());
    q.bind(child_ids_json);
    q.bind(node.span.map(|(a, _)| a as i64));
    q.bind(node.span.map(|(_, b)| b as i64));
    q.bind(node.page_start.map(|v| v as i32));
    q.bind(node.page_end.map(|v| v as i32));
    q.bind(keywords_json);
    q.bind(node.is_leaf);
    q.bind(node.created_at);
    q.bind(node.updated_at);
    q.bind(node.source_hash.to_vec());
    q.execute(&mut conn).await.map_err(|e| err("upsert", e))?;
    Ok(())
}

pub async fn get_node(pool: &MsPool, id: &NodeId) -> Result<Option<NodeRecord>> {
    let mut conn = pool.get().await.map_err(|e| err("get conn", e))?;
    let sql = format!("SELECT {NODE_COLS} FROM pagebridge_nodes WHERE node_id = @P1");
    let mut q = Query::new(sql);
    q.bind(id.as_str());
    let stream = q.query(&mut conn).await.map_err(|e| err("get", e))?;
    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| err("get rows", e))?;
    rows.first().map(row_to_node).transpose()
}

pub async fn children_summaries(pool: &MsPool, parent: &NodeId) -> Result<Vec<NodeSummary>> {
    let mut conn = pool.get().await.map_err(|e| err("children conn", e))?;
    let sql = "SELECT node_id, parent_id, title, level, routing_summary, is_leaf
               FROM pagebridge_nodes WHERE parent_id = @P1 ORDER BY node_id";
    let mut q = Query::new(sql);
    q.bind(parent.as_str());
    let stream = q.query(&mut conn).await.map_err(|e| err("children", e))?;
    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| err("children rows", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let node_id = req_str(row, 0, "node_id")?;
        let parent_id = opt_str(row, 1, "parent_id")?;
        let title = req_str(row, 2, "title")?;
        let level = req_u8(row, 3, "level")?;
        let routing_summary = req_str(row, 4, "routing_summary")?;
        let is_leaf = req_bool(row, 5, "is_leaf")?;
        out.push(NodeSummary {
            node_id: NodeId::new(node_id)?,
            parent_id: parent_id.map(NodeId::new).transpose()?,
            title,
            level: level_from_i32(i32::from(level)),
            routing_summary,
            is_leaf,
        });
    }
    Ok(out)
}

pub async fn children_records(pool: &MsPool, parent: &NodeId) -> Result<Vec<NodeRecord>> {
    let mut conn = pool.get().await.map_err(|e| err("child rec conn", e))?;
    let sql =
        format!("SELECT {NODE_COLS} FROM pagebridge_nodes WHERE parent_id = @P1 ORDER BY node_id");
    let mut q = Query::new(sql);
    q.bind(parent.as_str());
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("child rec", e))?
        .into_first_result()
        .await
        .map_err(|e| err("child rec rows", e))?;
    rows.iter().map(row_to_node).collect()
}

pub async fn leaves_under(pool: &MsPool, root: &NodeId) -> Result<Vec<NodeId>> {
    let prefix = format!("{}/%", root.as_str());
    let mut conn = pool.get().await.map_err(|e| err("leaves conn", e))?;
    let sql = "SELECT node_id FROM pagebridge_nodes
               WHERE (node_id = @P1 OR node_id LIKE @P2) AND is_leaf = 1
               ORDER BY node_id";
    let mut q = Query::new(sql);
    q.bind(root.as_str());
    q.bind(prefix);
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("leaves", e))?
        .into_first_result()
        .await
        .map_err(|e| err("leaves rows", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(NodeId::new(req_str(row, 0, "node_id")?)?);
    }
    Ok(out)
}

pub async fn delete_document(pool: &MsPool, doc_id: &DocId) -> Result<()> {
    let mut conn = pool.get().await.map_err(|e| err("delete conn", e))?;
    conn.simple_query("BEGIN TRANSACTION")
        .await
        .map_err(|e| err("begin", e))?
        .into_results()
        .await
        .map_err(|e| err("begin rows", e))?;
    for sql in [
        "DELETE FROM pagebridge_nodes WHERE doc_id = @P1",
        "DELETE FROM pagebridge_raw WHERE doc_id = @P1",
        "DELETE FROM pagebridge_docs WHERE doc_id = @P1",
    ] {
        let mut q = Query::new(sql);
        q.bind(doc_id.as_str());
        q.execute(&mut conn)
            .await
            .map_err(|e| err("delete row", e))?;
    }
    conn.simple_query("COMMIT TRANSACTION")
        .await
        .map_err(|e| err("commit", e))?
        .into_results()
        .await
        .map_err(|e| err("commit rows", e))?;
    Ok(())
}

pub async fn list_documents(pool: &MsPool) -> Result<Vec<DocumentEntry>> {
    let mut conn = pool.get().await.map_err(|e| err("list conn", e))?;
    let rows = conn
        .simple_query(
            "SELECT doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count
             FROM pagebridge_docs ORDER BY doc_id",
        )
        .await
        .map_err(|e| err("list", e))?
        .into_first_result()
        .await
        .map_err(|e| err("list rows", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(DocumentEntry {
            doc_id: DocId::new(req_str(row, 0, "doc_id")?)?,
            title: req_str(row, 1, "title")?,
            source_kind: req_str(row, 2, "source_kind")?,
            ingested_at: req_i64(row, 3, "ingested_at")?,
            root_node_id: NodeId::new(req_str(row, 4, "root_node_id")?)?,
            leaf_count: req_i32(row, 5, "leaf_count")? as u32,
            byte_count: req_i64(row, 6, "byte_count")? as u64,
            raw_text_hash: None,
            structural_hash: None,
        });
    }
    Ok(out)
}

pub async fn upsert_document(pool: &MsPool, doc: &DocumentEntry) -> Result<()> {
    let mut conn = pool.get().await.map_err(|e| err("upsert doc conn", e))?;
    let sql = "MERGE INTO pagebridge_docs AS t
        USING (SELECT @P1 AS doc_id) AS s
        ON t.doc_id = s.doc_id
        WHEN MATCHED THEN UPDATE SET
            title = @P2, source_kind = @P3, ingested_at = @P4,
            root_node_id = @P5, leaf_count = @P6, byte_count = @P7
        WHEN NOT MATCHED THEN INSERT
            (doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count)
            VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7);";
    let mut q = Query::new(sql);
    q.bind(doc.doc_id.as_str());
    q.bind(doc.title.as_str());
    q.bind(doc.source_kind.as_str());
    q.bind(doc.ingested_at);
    q.bind(doc.root_node_id.as_str());
    q.bind(doc.leaf_count as i32);
    q.bind(doc.byte_count as i64);
    q.execute(&mut conn)
        .await
        .map_err(|e| err("upsert doc", e))?;
    Ok(())
}

pub async fn search(
    pool: &MsPool,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = pool.get().await.map_err(|e| err("search conn", e))?;
    // Default to a LIKE-based search so this works even without full-text
    // search installed. Callers with FT available can use CONTAINSTABLE
    // directly via raw connections.
    let like = format!("%{}%", query.replace(['%', '_'], ""));
    let sql = if doc.is_some() {
        "SELECT TOP (@P3) node_id, doc_id, title,
                CAST(1.0 AS REAL) AS score
         FROM pagebridge_nodes
         WHERE is_leaf = 1 AND doc_id = @P2
           AND (title LIKE @P1 OR routing_summary LIKE @P1 OR summary LIKE @P1)
         ORDER BY LEN(title) ASC"
    } else {
        "SELECT TOP (@P2) node_id, doc_id, title,
                CAST(1.0 AS REAL) AS score
         FROM pagebridge_nodes
         WHERE is_leaf = 1
           AND (title LIKE @P1 OR routing_summary LIKE @P1 OR summary LIKE @P1)
         ORDER BY LEN(title) ASC"
    };
    let mut q = Query::new(sql);
    q.bind(like);
    if let Some(d) = doc {
        q.bind(d.as_str());
    }
    q.bind(i32::try_from(limit).unwrap_or(i32::MAX));
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("search", e))?
        .into_first_result()
        .await
        .map_err(|e| err("search rows", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let node_id = req_str(row, 0, "node_id")?;
        let doc_id = req_str(row, 1, "doc_id")?;
        let title = req_str(row, 2, "title")?;
        let score: f32 = row
            .try_get::<f32, _>(3)
            .map_err(|e| err("score", e))?
            .unwrap_or(0.0);
        out.push(SearchHit {
            node_id: NodeId::new(node_id)?,
            doc_id: DocId::new(doc_id)?,
            title,
            score,
        });
    }
    Ok(out)
}

pub async fn put_raw(
    pool: &MsPool,
    doc_id: &DocId,
    data: &[u8],
    chunk_limit: usize,
) -> Result<u64> {
    let mut conn = pool.get().await.map_err(|e| err("raw conn", e))?;
    let sql =
        "SELECT COALESCE(MAX(offset_start + length), 0) FROM pagebridge_raw WHERE doc_id = @P1";
    let mut q = Query::new(sql);
    q.bind(doc_id.as_str());
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("raw max", e))?
        .into_first_result()
        .await
        .map_err(|e| err("raw max rows", e))?;
    let start_u64 = if let Some(row) = rows.first() {
        row.try_get::<i64, _>(0)
            .map_err(|e| err("raw max col", e))?
            .unwrap_or(0) as u64
    } else {
        0
    };

    let mut written = 0usize;
    while written < data.len() {
        let take = (data.len() - written).min(chunk_limit);
        let chunk = data[written..written + take].to_vec();
        let chunk_off = start_u64 + written as u64;
        let mut q = Query::new(
            "INSERT INTO pagebridge_raw (doc_id, offset_start, length, data)
             VALUES (@P1, @P2, @P3, @P4)",
        );
        q.bind(doc_id.as_str());
        q.bind(chunk_off as i64);
        q.bind(i32::try_from(chunk.len()).unwrap_or(i32::MAX));
        q.bind(chunk);
        q.execute(&mut conn)
            .await
            .map_err(|e| err("insert raw", e))?;
        written += take;
    }
    Ok(start_u64)
}

pub async fn read_raw_span(pool: &MsPool, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
    if span.0 > span.1 {
        return Err(PagebridgeError::InvalidArgument(format!(
            "span {span:?} start > end"
        )));
    }
    let mut conn = pool.get().await.map_err(|e| err("raw read conn", e))?;
    let sql = "SELECT offset_start, data FROM pagebridge_raw
               WHERE doc_id = @P1 AND offset_start + length > @P2 AND offset_start < @P3
               ORDER BY offset_start";
    let mut q = Query::new(sql);
    q.bind(doc_id.as_str());
    q.bind(span.0 as i64);
    q.bind(span.1 as i64);
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("raw read", e))?
        .into_first_result()
        .await
        .map_err(|e| err("raw read rows", e))?;
    let mut out = Vec::with_capacity((span.1 - span.0) as usize);
    for row in &rows {
        let ofs = req_i64(row, 0, "offset_start")?;
        let data = req_bytes(row, 1, "data")?;
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

pub async fn get_summary_cache(
    pool: &MsPool,
    hash: &[u8; 32],
) -> Result<Option<SummaryCacheEntry>> {
    let mut conn = pool.get().await.map_err(|e| err("cache conn", e))?;
    let mut q = Query::new("SELECT entry FROM pagebridge_summary_cache WHERE source_hash = @P1");
    q.bind(hash.to_vec());
    let rows = q
        .query(&mut conn)
        .await
        .map_err(|e| err("cache get", e))?
        .into_first_result()
        .await
        .map_err(|e| err("cache get rows", e))?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let blob = req_bytes(row, 0, "entry")?;
    let entry: SummaryCacheEntry =
        serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
    Ok(Some(entry))
}

pub async fn upsert_summary_cache(
    pool: &MsPool,
    hash: &[u8; 32],
    entry: &SummaryCacheEntry,
) -> Result<()> {
    let blob = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
    let mut conn = pool.get().await.map_err(|e| err("cache upsert conn", e))?;
    let sql = "MERGE INTO pagebridge_summary_cache AS t
        USING (SELECT @P1 AS source_hash) AS s
        ON t.source_hash = s.source_hash
        WHEN MATCHED THEN UPDATE SET entry = @P2
        WHEN NOT MATCHED THEN INSERT (source_hash, entry) VALUES (@P1, @P2);";
    let mut q = Query::new(sql);
    q.bind(hash.to_vec());
    q.bind(blob);
    q.execute(&mut conn)
        .await
        .map_err(|e| err("upsert cache", e))?;
    Ok(())
}

pub async fn stats(pool: &MsPool) -> Result<AdapterStats> {
    let mut conn = pool.get().await.map_err(|e| err("stats conn", e))?;
    let rows = conn
        .simple_query(
            "SELECT
                (SELECT COUNT_BIG(*) FROM pagebridge_nodes) AS nodes,
                (SELECT COUNT_BIG(*) FROM pagebridge_docs) AS docs,
                (SELECT COALESCE(SUM(CAST(length AS BIGINT)), 0) FROM pagebridge_raw) AS raw,
                (SELECT COUNT_BIG(*) FROM pagebridge_summary_cache) AS cache",
        )
        .await
        .map_err(|e| err("stats", e))?
        .into_first_result()
        .await
        .map_err(|e| err("stats rows", e))?;
    let first = rows.first().ok_or_else(|| err("stats", "no row"))?;
    let nodes = first
        .try_get::<i64, _>(0)
        .map_err(|e| err("nodes", e))?
        .unwrap_or(0);
    let docs = first
        .try_get::<i64, _>(1)
        .map_err(|e| err("docs", e))?
        .unwrap_or(0);
    let raw = first
        .try_get::<i64, _>(2)
        .map_err(|e| err("raw", e))?
        .unwrap_or(0);
    let cache = first
        .try_get::<i64, _>(3)
        .map_err(|e| err("cache", e))?
        .unwrap_or(0);
    Ok(AdapterStats {
        node_count: nodes as u64,
        document_count: docs as u64,
        raw_bytes: raw as u64,
        summary_cache_entries: cache as u64,
    })
}
