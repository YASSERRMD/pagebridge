//! Statement bodies for the MySQL/MariaDB adapter, broken out so `lib.rs` stays
//! readable and each commit can land a focused diff.

use mysql_async::prelude::*;
use mysql_async::{from_value_opt, params, Pool, Row, Value};

use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{
    AdapterStats, DocumentEntry, DocumentType, SearchHit, SummaryCacheEntry,
};

use crate::{err, level_from_i32, level_to_i32};

fn col<T: FromValue>(row: &mut Row, idx: usize, label: &str) -> Result<T> {
    let v: Value = row
        .take_opt::<Value, _>(idx)
        .ok_or_else(|| err("col missing", label))?
        .map_err(|e| err(label, e))?;
    from_value_opt::<T>(v).map_err(|e| err(label, e))
}

fn row_to_node(mut row: Row) -> Result<NodeRecord> {
    let node_id: String = col(&mut row, 0, "node_id")?;
    let doc_id: String = col(&mut row, 1, "doc_id")?;
    let parent_id: Option<String> = col(&mut row, 2, "parent_id")?;
    let title: String = col(&mut row, 3, "title")?;
    let level: u8 = col(&mut row, 4, "level")?;
    let routing_summary: String = col(&mut row, 5, "routing_summary")?;
    let summary: String = col(&mut row, 6, "summary")?;
    let child_ids_json: String = col(&mut row, 7, "child_ids")?;
    let span_start: Option<i64> = col(&mut row, 8, "span_start")?;
    let span_end: Option<i64> = col(&mut row, 9, "span_end")?;
    let page_start: Option<i32> = col(&mut row, 10, "page_start")?;
    let page_end: Option<i32> = col(&mut row, 11, "page_end")?;
    let keywords_json: String = col(&mut row, 12, "keywords")?;
    let is_leaf: i8 = col(&mut row, 13, "is_leaf")?;
    let created_at: i64 = col(&mut row, 14, "created_at")?;
    let updated_at: i64 = col(&mut row, 15, "updated_at")?;
    let source_hash_blob: Vec<u8> = col(&mut row, 16, "source_hash")?;
    let canonical_section: Option<String> = col(&mut row, 17, "canonical_section")?;
    let section_aliases_json: String = col(&mut row, 18, "section_aliases")?;

    let mut hash = [0u8; 32];
    let len = source_hash_blob.len().min(32);
    hash[..len].copy_from_slice(&source_hash_blob[..len]);

    let child_strs: Vec<String> =
        serde_json::from_str(&child_ids_json).map_err(|e| err("decode child_ids", e))?;
    let keywords: Vec<String> =
        serde_json::from_str(&keywords_json).map_err(|e| err("decode keywords", e))?;
    let section_aliases: Vec<String> = serde_json::from_str(&section_aliases_json)
        .map_err(|e| err("decode section_aliases", e))?;
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
        is_leaf: is_leaf != 0,
        created_at,
        updated_at,
        source_hash: hash,
        canonical_section,
        section_aliases,
    })
}

pub async fn upsert_node(pool: &Pool, node: &NodeRecord) -> Result<()> {
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

    let section_aliases_json =
        serde_json::to_string(&node.section_aliases).map_err(|e| err("encode aliases", e))?;
    let mut conn = pool.get_conn().await.map_err(|e| err("upsert conn", e))?;
    r"INSERT INTO pagebridge_nodes
        (node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
         span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
         updated_at, source_hash, canonical_section, section_aliases)
      VALUES (:node_id, :doc_id, :parent_id, :title, :level, :routing_summary, :summary, :child_ids,
              :span_start, :span_end, :page_start, :page_end, :keywords, :is_leaf, :created_at,
              :updated_at, :source_hash, :canonical_section, :section_aliases)
      ON DUPLICATE KEY UPDATE
        doc_id = VALUES(doc_id),
        parent_id = VALUES(parent_id),
        title = VALUES(title),
        level = VALUES(level),
        routing_summary = VALUES(routing_summary),
        summary = VALUES(summary),
        child_ids = VALUES(child_ids),
        span_start = VALUES(span_start),
        span_end = VALUES(span_end),
        page_start = VALUES(page_start),
        page_end = VALUES(page_end),
        keywords = VALUES(keywords),
        is_leaf = VALUES(is_leaf),
        created_at = VALUES(created_at),
        updated_at = VALUES(updated_at),
        source_hash = VALUES(source_hash),
        canonical_section = VALUES(canonical_section),
        section_aliases = VALUES(section_aliases)"
        .with(params! {
            "node_id" => node.node_id.as_str(),
            "doc_id" => node.doc_id.as_str(),
            "parent_id" => node.parent_id.as_ref().map(|p| p.as_str().to_owned()),
            "title" => node.title.clone(),
            "level" => u8::try_from(level_to_i32(node.level)).unwrap_or(0),
            "routing_summary" => node.routing_summary.clone(),
            "summary" => node.summary.clone(),
            "child_ids" => child_ids_json,
            "span_start" => node.span.map(|(a, _)| a as i64),
            "span_end" => node.span.map(|(_, b)| b as i64),
            "page_start" => node.page_start.map(|v| v as i32),
            "page_end" => node.page_end.map(|v| v as i32),
            "keywords" => keywords_json,
            "is_leaf" => i8::from(node.is_leaf),
            "created_at" => node.created_at,
            "updated_at" => node.updated_at,
            "source_hash" => node.source_hash.to_vec(),
            "canonical_section" => node.canonical_section.clone(),
            "section_aliases" => section_aliases_json,
        })
        .ignore(&mut conn)
        .await
        .map_err(|e| err("upsert_node", e))?;
    Ok(())
}

pub async fn get_node(pool: &Pool, id: &NodeId) -> Result<Option<NodeRecord>> {
    let mut conn = pool.get_conn().await.map_err(|e| err("get conn", e))?;
    let row: Option<Row> = "SELECT
        node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
        span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
        updated_at, source_hash, canonical_section, IFNULL(section_aliases, '[]') AS section_aliases
      FROM pagebridge_nodes WHERE node_id = :node_id"
        .with(params! { "node_id" => id.as_str() })
        .first(&mut conn)
        .await
        .map_err(|e| err("get_node", e))?;
    row.map(row_to_node).transpose()
}

pub async fn children_summaries(pool: &Pool, parent: &NodeId) -> Result<Vec<NodeSummary>> {
    let mut conn = pool.get_conn().await.map_err(|e| err("children conn", e))?;
    let rows: Vec<(String, Option<String>, String, u8, String, i8)> =
        "SELECT node_id, parent_id, title, level, routing_summary, is_leaf
         FROM pagebridge_nodes WHERE parent_id = :parent ORDER BY node_id"
            .with(params! { "parent" => parent.as_str() })
            .fetch(&mut conn)
            .await
            .map_err(|e| err("children_summaries", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for (node_id, parent_id, title, level, routing_summary, is_leaf) in rows {
        out.push(NodeSummary {
            node_id: NodeId::new(node_id)?,
            parent_id: parent_id.map(NodeId::new).transpose()?,
            title,
            level: level_from_i32(i32::from(level)),
            routing_summary,
            is_leaf: is_leaf != 0,
        });
    }
    Ok(out)
}

pub async fn children_records(pool: &Pool, parent: &NodeId) -> Result<Vec<NodeRecord>> {
    let mut conn = pool
        .get_conn()
        .await
        .map_err(|e| err("children rec conn", e))?;
    let rows: Vec<Row> = "SELECT
        node_id, doc_id, parent_id, title, level, routing_summary, summary, child_ids,
        span_start, span_end, page_start, page_end, keywords, is_leaf, created_at,
        updated_at, source_hash, canonical_section, IFNULL(section_aliases, '[]') AS section_aliases
      FROM pagebridge_nodes WHERE parent_id = :parent ORDER BY node_id"
        .with(params! { "parent" => parent.as_str() })
        .fetch(&mut conn)
        .await
        .map_err(|e| err("children_records", e))?;
    rows.into_iter().map(row_to_node).collect()
}

pub async fn leaves_under(pool: &Pool, root: &NodeId) -> Result<Vec<NodeId>> {
    let prefix = format!("{}/%", root.as_str());
    let mut conn = pool.get_conn().await.map_err(|e| err("leaves conn", e))?;
    let rows: Vec<String> = "SELECT node_id FROM pagebridge_nodes
         WHERE (node_id = :exact OR node_id LIKE :pfx) AND is_leaf = 1
         ORDER BY node_id"
        .with(params! { "exact" => root.as_str(), "pfx" => prefix })
        .fetch(&mut conn)
        .await
        .map_err(|e| err("leaves_under", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for id in rows {
        out.push(NodeId::new(id)?);
    }
    Ok(out)
}

pub async fn delete_document(pool: &Pool, doc_id: &DocId) -> Result<()> {
    let mut conn = pool.get_conn().await.map_err(|e| err("delete conn", e))?;
    let mut tx = conn
        .start_transaction(mysql_async::TxOpts::default())
        .await
        .map_err(|e| err("tx", e))?;
    "DELETE FROM pagebridge_nodes WHERE doc_id = :d"
        .with(params! { "d" => doc_id.as_str() })
        .ignore(&mut tx)
        .await
        .map_err(|e| err("delete nodes", e))?;
    "DELETE FROM pagebridge_raw WHERE doc_id = :d"
        .with(params! { "d" => doc_id.as_str() })
        .ignore(&mut tx)
        .await
        .map_err(|e| err("delete raw", e))?;
    "DELETE FROM pagebridge_docs WHERE doc_id = :d"
        .with(params! { "d" => doc_id.as_str() })
        .ignore(&mut tx)
        .await
        .map_err(|e| err("delete docs", e))?;
    tx.commit().await.map_err(|e| err("commit", e))?;
    Ok(())
}

pub async fn list_documents(pool: &Pool) -> Result<Vec<DocumentEntry>> {
    type DocRow = (
        String,
        String,
        String,
        i64,
        String,
        i32,
        i64,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<String>,
    );
    let mut conn = pool.get_conn().await.map_err(|e| err("list conn", e))?;
    let rows: Vec<DocRow> =
        "SELECT doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count,
                raw_text_hash, structural_hash, document_type
         FROM pagebridge_docs ORDER BY doc_id"
            .fetch(&mut conn)
            .await
            .map_err(|e| err("list_documents", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for (
        doc_id,
        title,
        source_kind,
        ingested_at,
        root_node_id,
        leaf_count,
        byte_count,
        raw_hash_blob,
        struct_hash_blob,
        doc_type_str,
    ) in rows
    {
        let raw_text_hash = raw_hash_blob.and_then(|b| {
            if b.len() == 32 {
                let mut a = [0u8; 32];
                a.copy_from_slice(&b);
                Some(a)
            } else {
                None
            }
        });
        let structural_hash = struct_hash_blob.and_then(|b| {
            if b.len() == 32 {
                let mut a = [0u8; 32];
                a.copy_from_slice(&b);
                Some(a)
            } else {
                None
            }
        });
        out.push(DocumentEntry {
            doc_id: DocId::new(doc_id)?,
            title,
            source_kind,
            ingested_at,
            root_node_id: NodeId::new(root_node_id)?,
            leaf_count: leaf_count as u32,
            byte_count: byte_count as u64,
            raw_text_hash,
            structural_hash,
            document_type: doc_type_str.as_deref().and_then(DocumentType::parse_tag),
        });
    }
    Ok(out)
}

pub async fn upsert_document(pool: &Pool, doc: &DocumentEntry) -> Result<()> {
    let mut conn = pool
        .get_conn()
        .await
        .map_err(|e| err("upsert doc conn", e))?;
    "INSERT INTO pagebridge_docs
        (doc_id, title, source_kind, ingested_at, root_node_id, leaf_count, byte_count,
         raw_text_hash, structural_hash, document_type)
     VALUES (:d, :t, :k, :i, :r, :lc, :bc, :rth, :sh, :dt)
     ON DUPLICATE KEY UPDATE
        title = VALUES(title),
        source_kind = VALUES(source_kind),
        ingested_at = VALUES(ingested_at),
        root_node_id = VALUES(root_node_id),
        leaf_count = VALUES(leaf_count),
        byte_count = VALUES(byte_count),
        raw_text_hash = VALUES(raw_text_hash),
        structural_hash = VALUES(structural_hash),
        document_type = VALUES(document_type)"
        .with(params! {
            "d" => doc.doc_id.as_str(),
            "t" => doc.title.clone(),
            "k" => doc.source_kind.clone(),
            "i" => doc.ingested_at,
            "r" => doc.root_node_id.as_str(),
            "lc" => doc.leaf_count as i32,
            "bc" => doc.byte_count as i64,
            "rth" => doc.raw_text_hash.map(|h| h.to_vec()),
            "sh" => doc.structural_hash.map(|h| h.to_vec()),
            "dt" => doc.document_type.map(|dt| dt.as_str().to_owned()),
        })
        .ignore(&mut conn)
        .await
        .map_err(|e| err("upsert_document", e))?;
    Ok(())
}

pub async fn search(
    pool: &Pool,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = pool.get_conn().await.map_err(|e| err("search conn", e))?;
    let rows: Vec<(String, String, String, f64)> = if let Some(d) = doc {
        "SELECT node_id, doc_id, title,
                MATCH(title, routing_summary, summary) AGAINST (:q IN NATURAL LANGUAGE MODE) AS score
         FROM pagebridge_nodes
         WHERE is_leaf = 1 AND doc_id = :doc
           AND MATCH(title, routing_summary, summary) AGAINST (:q IN NATURAL LANGUAGE MODE) > 0
         ORDER BY score DESC LIMIT :lim"
            .with(params! { "q" => query, "doc" => d.as_str(), "lim" => limit as i64 })
            .fetch(&mut conn)
            .await
            .map_err(|e| err("search", e))?
    } else {
        "SELECT node_id, doc_id, title,
                MATCH(title, routing_summary, summary) AGAINST (:q IN NATURAL LANGUAGE MODE) AS score
         FROM pagebridge_nodes
         WHERE is_leaf = 1
           AND MATCH(title, routing_summary, summary) AGAINST (:q IN NATURAL LANGUAGE MODE) > 0
         ORDER BY score DESC LIMIT :lim"
            .with(params! { "q" => query, "lim" => limit as i64 })
            .fetch(&mut conn)
            .await
            .map_err(|e| err("search", e))?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (node_id, doc_id, title, score) in rows {
        out.push(SearchHit {
            node_id: NodeId::new(node_id)?,
            doc_id: DocId::new(doc_id)?,
            title,
            score: score as f32,
        });
    }
    Ok(out)
}

pub async fn put_raw(pool: &Pool, doc_id: &DocId, data: &[u8], chunk_limit: usize) -> Result<u64> {
    let mut conn = pool.get_conn().await.map_err(|e| err("raw conn", e))?;
    let end: Option<i64> = "SELECT COALESCE(MAX(offset_start + length), 0)
         FROM pagebridge_raw WHERE doc_id = :d"
        .with(params! { "d" => doc_id.as_str() })
        .first(&mut conn)
        .await
        .map_err(|e| err("raw max", e))?;
    let start_u64 = end.unwrap_or(0) as u64;
    let mut written = 0usize;
    while written < data.len() {
        let take = (data.len() - written).min(chunk_limit);
        let chunk = data[written..written + take].to_vec();
        let chunk_off = start_u64 + written as u64;
        "INSERT INTO pagebridge_raw (doc_id, offset_start, length, data)
         VALUES (:d, :o, :l, :data)"
            .with(params! {
                "d" => doc_id.as_str(),
                "o" => chunk_off as i64,
                "l" => chunk.len() as i32,
                "data" => chunk,
            })
            .ignore(&mut conn)
            .await
            .map_err(|e| err("insert raw", e))?;
        written += take;
    }
    Ok(start_u64)
}

pub async fn read_raw_span(pool: &Pool, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
    if span.0 > span.1 {
        return Err(PagebridgeError::InvalidArgument(format!(
            "span {span:?} start > end"
        )));
    }
    let mut conn = pool.get_conn().await.map_err(|e| err("raw read conn", e))?;
    let rows: Vec<(i64, Vec<u8>)> = "SELECT offset_start, data FROM pagebridge_raw
         WHERE doc_id = :d AND offset_start + length > :a AND offset_start < :b
         ORDER BY offset_start"
        .with(params! {
            "d" => doc_id.as_str(),
            "a" => span.0 as i64,
            "b" => span.1 as i64,
        })
        .fetch(&mut conn)
        .await
        .map_err(|e| err("read raw", e))?;
    let mut out = Vec::with_capacity((span.1 - span.0) as usize);
    for (ofs, data) in rows {
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

pub async fn get_summary_cache(pool: &Pool, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
    let mut conn = pool.get_conn().await.map_err(|e| err("cache conn", e))?;
    let row: Option<Vec<u8>> = "SELECT entry FROM pagebridge_summary_cache WHERE source_hash = :h"
        .with(params! { "h" => hash.to_vec() })
        .first(&mut conn)
        .await
        .map_err(|e| err("get_summary_cache", e))?;
    let Some(blob) = row else { return Ok(None) };
    let entry: SummaryCacheEntry =
        serde_json::from_slice(&blob).map_err(|e| err("decode cache", e))?;
    Ok(Some(entry))
}

pub async fn upsert_summary_cache(
    pool: &Pool,
    hash: &[u8; 32],
    entry: &SummaryCacheEntry,
) -> Result<()> {
    let blob = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
    let mut conn = pool
        .get_conn()
        .await
        .map_err(|e| err("cache upsert conn", e))?;
    "INSERT INTO pagebridge_summary_cache (source_hash, entry)
     VALUES (:h, :e)
     ON DUPLICATE KEY UPDATE entry = VALUES(entry)"
        .with(params! { "h" => hash.to_vec(), "e" => blob })
        .ignore(&mut conn)
        .await
        .map_err(|e| err("upsert cache", e))?;
    Ok(())
}

pub async fn stats(pool: &Pool) -> Result<AdapterStats> {
    let mut conn = pool.get_conn().await.map_err(|e| err("stats conn", e))?;
    let nodes: i64 = "SELECT COUNT(*) FROM pagebridge_nodes"
        .first(&mut conn)
        .await
        .map_err(|e| err("count nodes", e))?
        .unwrap_or(0);
    let docs: i64 = "SELECT COUNT(*) FROM pagebridge_docs"
        .first(&mut conn)
        .await
        .map_err(|e| err("count docs", e))?
        .unwrap_or(0);
    let raw: i64 = "SELECT COALESCE(SUM(length), 0) FROM pagebridge_raw"
        .first(&mut conn)
        .await
        .map_err(|e| err("sum raw", e))?
        .unwrap_or(0);
    let cache: i64 = "SELECT COUNT(*) FROM pagebridge_summary_cache"
        .first(&mut conn)
        .await
        .map_err(|e| err("count cache", e))?
        .unwrap_or(0);
    Ok(AdapterStats {
        node_count: nodes as u64,
        document_count: docs as u64,
        raw_bytes: raw as u64,
        summary_cache_entries: cache as u64,
    })
}
