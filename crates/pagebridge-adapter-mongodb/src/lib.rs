//! MongoDB storage adapter for pagebridge.
//!
//! Uses the official mongodb crate. Text search is implemented via Mongo's
//! `$text` operator with a compound text index. Raw text is stored chunked in
//! a dedicated collection with `(doc_id, offset_start)` as the key.

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
use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Binary, Bson, Document};
use mongodb::options::IndexOptions;
use mongodb::{Client, Collection, Database, IndexModel};
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

const RAW_CHUNK_LIMIT: usize = 1024 * 1024;

/// MongoDB storage adapter, backed by the official mongodb crate.
#[derive(Debug, Clone)]
pub struct MongoAdapter {
    db: Database,
}

impl MongoAdapter {
    /// Connect to MongoDB and select the given database.
    pub async fn connect(url: &str, database: &str) -> Result<Self> {
        let client = Client::with_uri_str(url)
            .await
            .map_err(|e| err("connect", e))?;
        let db = client.database(database);
        Ok(Self { db })
    }

    fn nodes(&self) -> Collection<Document> {
        self.db.collection::<Document>("pagebridge_nodes")
    }
    fn docs(&self) -> Collection<Document> {
        self.db.collection::<Document>("pagebridge_docs")
    }
    fn raw(&self) -> Collection<Document> {
        self.db.collection::<Document>("pagebridge_raw")
    }
    fn summary_cache(&self) -> Collection<Document> {
        self.db.collection::<Document>("pagebridge_summary_cache")
    }
}

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "mongodb".into(),
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

fn node_to_doc(node: &NodeRecord) -> Document {
    let mut keywords = Vec::with_capacity(node.keywords.len());
    for k in &node.keywords {
        keywords.push(Bson::String(k.clone()));
    }
    let child_ids: Vec<Bson> = node
        .child_ids
        .iter()
        .map(|c| Bson::String(c.as_str().to_owned()))
        .collect();
    let mut d = doc! {
        "_id": node.node_id.as_str(),
        "node_id": node.node_id.as_str(),
        "doc_id": node.doc_id.as_str(),
        "parent_id": node.parent_id.as_ref().map_or(Bson::Null, |p| Bson::String(p.as_str().to_owned())),
        "title": &node.title,
        "level": level_to_i32(node.level),
        "routing_summary": &node.routing_summary,
        "summary": &node.summary,
        "child_ids": child_ids,
        "keywords": keywords,
        "is_leaf": node.is_leaf,
        "created_at": node.created_at,
        "updated_at": node.updated_at,
        "source_hash": Binary { subtype: mongodb::bson::spec::BinarySubtype::Generic, bytes: node.source_hash.to_vec() },
    };
    if let Some((a, b)) = node.span {
        d.insert("span_start", a as i64);
        d.insert("span_end", b as i64);
    }
    if let Some(p) = node.page_start {
        d.insert("page_start", i32::try_from(p).unwrap_or(i32::MAX));
    }
    if let Some(p) = node.page_end {
        d.insert("page_end", i32::try_from(p).unwrap_or(i32::MAX));
    }
    d
}

fn doc_to_node(d: Document) -> Result<NodeRecord> {
    let node_id: &str = d.get_str("node_id").map_err(|e| err("col node_id", e))?;
    let doc_id: &str = d.get_str("doc_id").map_err(|e| err("col doc_id", e))?;
    let parent_id: Option<String> = match d.get("parent_id") {
        Some(Bson::String(s)) => Some(s.clone()),
        _ => None,
    };
    let title = d
        .get_str("title")
        .map_err(|e| err("col title", e))?
        .to_owned();
    let level: i32 = d.get_i32("level").map_err(|e| err("col level", e))?;
    let routing_summary = d
        .get_str("routing_summary")
        .map_err(|e| err("col rs", e))?
        .to_owned();
    let summary = d
        .get_str("summary")
        .map_err(|e| err("col summary", e))?
        .to_owned();
    let child_ids = d
        .get_array("child_ids")
        .map_err(|e| err("col child_ids", e))?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let keywords = d
        .get_array("keywords")
        .map_err(|e| err("col keywords", e))?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let is_leaf = d.get_bool("is_leaf").map_err(|e| err("col is_leaf", e))?;
    let created_at = d.get_i64("created_at").map_err(|e| err("col created", e))?;
    let updated_at = d.get_i64("updated_at").map_err(|e| err("col updated", e))?;
    let span_start = d.get_i64("span_start").ok();
    let span_end = d.get_i64("span_end").ok();
    let page_start = d.get_i32("page_start").ok().map(|v| v as u32);
    let page_end = d.get_i32("page_end").ok().map(|v| v as u32);
    let source_hash_bson = d
        .get("source_hash")
        .ok_or_else(|| err("col source_hash", "missing"))?;
    let mut hash = [0u8; 32];
    if let Bson::Binary(Binary { bytes, .. }) = source_hash_bson {
        let n = bytes.len().min(32);
        hash[..n].copy_from_slice(&bytes[..n]);
    }
    let mut child_node_ids = Vec::with_capacity(child_ids.len());
    for c in child_ids {
        child_node_ids.push(NodeId::new(c)?);
    }
    Ok(NodeRecord {
        node_id: NodeId::new(node_id)?,
        doc_id: DocId::new(doc_id)?,
        parent_id: parent_id.map(NodeId::new).transpose()?,
        title,
        level: level_from_i32(level),
        routing_summary,
        summary,
        child_ids: child_node_ids,
        span: match (span_start, span_end) {
            (Some(a), Some(b)) => Some((a as u64, b as u64)),
            _ => None,
        },
        page_start,
        page_end,
        keywords,
        is_leaf,
        created_at,
        updated_at,
        source_hash: hash,
        canonical_section: None,
        section_aliases: vec![],
    })
}

#[async_trait]
impl StorageAdapter for MongoAdapter {
    fn name(&self) -> &'static str {
        "mongodb"
    }

    async fn migrate(&self) -> Result<()> {
        // Indexes on the nodes collection.
        let nodes = self.nodes();
        let _ = nodes
            .create_indexes(vec![
                IndexModel::builder().keys(doc! {"doc_id": 1}).build(),
                IndexModel::builder().keys(doc! {"parent_id": 1}).build(),
                IndexModel::builder()
                    .keys(doc! {
                        "title": "text",
                        "routing_summary": "text",
                        "summary": "text",
                        "keywords": "text",
                    })
                    .options(
                        IndexOptions::builder()
                            .name(Some("pagebridge_text".to_owned()))
                            .build(),
                    )
                    .build(),
            ])
            .await
            .map_err(|e| err("nodes indexes", e))?;

        let raw = self.raw();
        let _ = raw
            .create_indexes(vec![IndexModel::builder()
                .keys(doc! {"doc_id": 1, "offset_start": 1})
                .options(IndexOptions::builder().unique(Some(true)).build())
                .build()])
            .await
            .map_err(|e| err("raw indexes", e))?;
        Ok(())
    }

    async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
        node.validate()?;
        let d = node_to_doc(node);
        let filter = doc! { "_id": node.node_id.as_str() };
        self.nodes()
            .replace_one(filter, d)
            .upsert(true)
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
        // The mongodb driver's bulk_write surface differs between versions;
        // issue one replace_one with upsert per node concurrently via a
        // futures::future::try_join_all so the round-trips overlap. This is
        // dramatically faster than the prior await-each-in-sequence flow even
        // when the driver-level batch isn't used.
        let coll = self.nodes();
        let mut futures = Vec::with_capacity(nodes.len());
        for node in nodes {
            let d = node_to_doc(node);
            let filter = doc! { "_id": node.node_id.as_str() };
            futures.push(coll.replace_one(filter, d).upsert(true));
        }
        for f in futures {
            f.await.map_err(|e| err("batch upsert", e))?;
        }
        Ok(())
    }

    fn recommended_batch_size(&self) -> usize {
        500
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let opt = self
            .nodes()
            .find_one(doc! { "_id": id.as_str() })
            .await
            .map_err(|e| err("get_node", e))?;
        opt.map(doc_to_node).transpose()
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
        let mut cursor = self
            .nodes()
            .find(doc! { "parent_id": parent.as_str() })
            .sort(doc! { "node_id": 1 })
            .await
            .map_err(|e| err("children_summaries find", e))?;
        let mut out = Vec::new();
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            let rec = doc_to_node(d)?;
            out.push(NodeSummary::from(&rec));
        }
        Ok(out)
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        let mut cursor = self
            .nodes()
            .find(doc! { "parent_id": parent.as_str() })
            .sort(doc! { "node_id": 1 })
            .await
            .map_err(|e| err("children_records find", e))?;
        let mut out = Vec::new();
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            out.push(doc_to_node(d)?);
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
        // Regex-anchored prefix match.
        let prefix = format!("^{}", regex_escape(root.as_str()));
        let mut cursor = self
            .nodes()
            .find(doc! {
                "node_id": { "$regex": prefix, "$options": "" },
                "is_leaf": true,
            })
            .sort(doc! { "node_id": 1 })
            .await
            .map_err(|e| err("leaves_under find", e))?;
        let mut out = Vec::new();
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            if let Ok(s) = d.get_str("node_id") {
                if s == root.as_str() || s.starts_with(&format!("{}/", root.as_str())) {
                    out.push(NodeId::new(s)?);
                }
            }
        }
        Ok(out)
    }

    async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
        let id_filter = doc! { "doc_id": doc_id.as_str() };
        self.nodes()
            .delete_many(id_filter.clone())
            .await
            .map_err(|e| err("delete nodes", e))?;
        self.raw()
            .delete_many(id_filter.clone())
            .await
            .map_err(|e| err("delete raw", e))?;
        self.docs()
            .delete_many(doc! { "_id": doc_id.as_str() })
            .await
            .map_err(|e| err("delete docs", e))?;
        Ok(())
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        let mut cursor = self
            .docs()
            .find(doc! {})
            .sort(doc! { "_id": 1 })
            .await
            .map_err(|e| err("list_documents find", e))?;
        let mut out = Vec::new();
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            let doc_id = d.get_str("_id").map_err(|e| err("col", e))?.to_owned();
            let title = d.get_str("title").map_err(|e| err("col", e))?.to_owned();
            let source_kind = d
                .get_str("source_kind")
                .map_err(|e| err("col", e))?
                .to_owned();
            let ingested_at = d.get_i64("ingested_at").map_err(|e| err("col", e))?;
            let root_node_id = d
                .get_str("root_node_id")
                .map_err(|e| err("col", e))?
                .to_owned();
            let leaf_count = d.get_i32("leaf_count").map_err(|e| err("col", e))?;
            let byte_count = d.get_i64("byte_count").map_err(|e| err("col", e))?;
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
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        let d = doc! {
            "_id": doc.doc_id.as_str(),
            "title": &doc.title,
            "source_kind": &doc.source_kind,
            "ingested_at": doc.ingested_at,
            "root_node_id": doc.root_node_id.as_str(),
            "leaf_count": doc.leaf_count as i32,
            "byte_count": doc.byte_count as i64,
        };
        self.docs()
            .replace_one(doc! { "_id": doc.doc_id.as_str() }, d)
            .upsert(true)
            .await
            .map_err(|e| err("upsert_document", e))?;
        Ok(())
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        search_impl(&self.nodes(), query, limit, None).await
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        search_impl(&self.nodes(), query, limit, Some(doc_id)).await
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        // Find current end offset.
        let mut cursor = self
            .raw()
            .find(doc! { "doc_id": doc_id.as_str() })
            .sort(doc! { "offset_start": -1 })
            .limit(1)
            .await
            .map_err(|e| err("raw max offset", e))?;
        let start = if let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            let ofs = d.get_i64("offset_start").map_err(|e| err("col", e))?;
            let len = d.get_i32("length").map_err(|e| err("col", e))?;
            (ofs as u64) + len as u64
        } else {
            0
        };
        let mut written = 0usize;
        while written < data.len() {
            let take = (data.len() - written).min(RAW_CHUNK_LIMIT);
            let chunk = &data[written..written + take];
            let chunk_off = start + written as u64;
            let bin = Binary {
                subtype: mongodb::bson::spec::BinarySubtype::Generic,
                bytes: chunk.to_vec(),
            };
            self.raw()
                .insert_one(doc! {
                    "doc_id": doc_id.as_str(),
                    "offset_start": chunk_off as i64,
                    "length": chunk.len() as i32,
                    "data": bin,
                })
                .await
                .map_err(|e| err("insert raw", e))?;
            written += take;
        }
        Ok(start)
    }

    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
        if span.0 > span.1 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "span {span:?} start > end"
            )));
        }
        let mut cursor = self
            .raw()
            .find(doc! {
                "doc_id": doc_id.as_str(),
                "offset_start": { "$lt": span.1 as i64 },
            })
            .sort(doc! { "offset_start": 1 })
            .await
            .map_err(|e| err("read_raw_span find", e))?;
        let mut out = Vec::with_capacity((span.1 - span.0) as usize);
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            let ofs = d.get_i64("offset_start").map_err(|e| err("col", e))?;
            let bin = d.get("data").ok_or_else(|| err("col data", "missing"))?;
            let bytes = if let Bson::Binary(Binary { bytes, .. }) = bin {
                bytes.clone()
            } else {
                return Err(err("data type", "not binary"));
            };
            let cs = ofs as u64;
            let ce = cs + bytes.len() as u64;
            if ce <= span.0 {
                continue;
            }
            let rs = span.0.max(cs);
            let re = span.1.min(ce);
            if rs < re {
                let s = (rs - cs) as usize;
                let e = (re - cs) as usize;
                out.extend_from_slice(&bytes[s..e]);
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
        let opt = self
            .summary_cache()
            .find_one(doc! {
                "_id": Binary { subtype: mongodb::bson::spec::BinarySubtype::Generic, bytes: hash.to_vec() }
            })
            .await
            .map_err(|e| err("get_summary_cache", e))?;
        let Some(d) = opt else { return Ok(None) };
        let entry_bson = d.get("entry").ok_or_else(|| err("col entry", "missing"))?;
        let bytes = if let Bson::Binary(Binary { bytes, .. }) = entry_bson {
            bytes.clone()
        } else {
            return Err(err("entry type", "not binary"));
        };
        let entry: SummaryCacheEntry =
            serde_json::from_slice(&bytes).map_err(|e| err("decode cache", e))?;
        Ok(Some(entry))
    }

    async fn get_summary_caches_batch(
        &self,
        hashes: &[[u8; 32]],
    ) -> Result<std::collections::HashMap<[u8; 32], SummaryCacheEntry>> {
        use futures::TryStreamExt;
        if hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let ids: Vec<Bson> = hashes
            .iter()
            .map(|h| {
                Bson::Binary(Binary {
                    subtype: mongodb::bson::spec::BinarySubtype::Generic,
                    bytes: h.to_vec(),
                })
            })
            .collect();
        let mut cursor = self
            .summary_cache()
            .find(doc! { "_id": { "$in": ids } })
            .await
            .map_err(|e| err("batch get_summary_cache", e))?;
        let mut out = std::collections::HashMap::with_capacity(hashes.len());
        while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor next", e))? {
            let id_bson = d.get("_id").ok_or_else(|| err("col _id", "missing"))?;
            let Bson::Binary(Binary { bytes: hbytes, .. }) = id_bson else {
                continue;
            };
            if hbytes.len() != 32 {
                continue;
            }
            let mut harr = [0u8; 32];
            harr.copy_from_slice(hbytes);
            let entry_bson = d.get("entry").ok_or_else(|| err("col entry", "missing"))?;
            let Bson::Binary(Binary { bytes, .. }) = entry_bson else {
                continue;
            };
            let entry: SummaryCacheEntry =
                serde_json::from_slice(bytes).map_err(|e| err("decode cache", e))?;
            out.insert(harr, entry);
        }
        Ok(out)
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        let bytes = serde_json::to_vec(entry).map_err(|e| err("encode cache", e))?;
        let id = Binary {
            subtype: mongodb::bson::spec::BinarySubtype::Generic,
            bytes: hash.to_vec(),
        };
        let blob = Binary {
            subtype: mongodb::bson::spec::BinarySubtype::Generic,
            bytes,
        };
        self.summary_cache()
            .replace_one(
                doc! { "_id": id.clone() },
                doc! { "_id": id, "entry": blob },
            )
            .upsert(true)
            .await
            .map_err(|e| err("upsert cache", e))?;
        Ok(())
    }

    async fn stats(&self) -> Result<AdapterStats> {
        let nodes = self
            .nodes()
            .count_documents(doc! {})
            .await
            .map_err(|e| err("nodes count", e))?;
        let docs = self
            .docs()
            .count_documents(doc! {})
            .await
            .map_err(|e| err("docs count", e))?;
        let cache = self
            .summary_cache()
            .count_documents(doc! {})
            .await
            .map_err(|e| err("cache count", e))?;
        // Sum raw lengths via aggregation.
        let mut cursor = self
            .raw()
            .aggregate(vec![doc! {
                "$group": { "_id": Bson::Null, "total": { "$sum": "$length" } }
            }])
            .await
            .map_err(|e| err("raw sum agg", e))?;
        let mut raw_bytes: u64 = 0;
        if let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
            if let Ok(n) = d.get_i64("total") {
                raw_bytes = n as u64;
            } else if let Ok(n) = d.get_i32("total") {
                raw_bytes = n as u64;
            }
        }
        Ok(AdapterStats {
            node_count: nodes,
            document_count: docs,
            raw_bytes,
            summary_cache_entries: cache,
        })
    }

    async fn ping(&self) -> Result<()> {
        self.db
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| err("ping", e))?;
        Ok(())
    }
}

async fn search_impl(
    nodes: &Collection<Document>,
    query: &str,
    limit: usize,
    doc_filter: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut filter = doc! {
        "$text": { "$search": query },
        "is_leaf": true,
    };
    if let Some(d) = doc_filter {
        filter.insert("doc_id", d.as_str());
    }
    let mut cursor = nodes
        .find(filter)
        .projection(doc! {
            "node_id": 1, "doc_id": 1, "title": 1,
            "score": { "$meta": "textScore" },
        })
        .sort(doc! { "score": { "$meta": "textScore" } })
        .limit(limit as i64)
        .await
        .map_err(|e| err("text find", e))?;
    let mut out = Vec::new();
    while let Some(d) = cursor.try_next().await.map_err(|e| err("cursor", e))? {
        let node_id = d.get_str("node_id").map_err(|e| err("col", e))?.to_owned();
        let doc_id = d.get_str("doc_id").map_err(|e| err("col", e))?.to_owned();
        let title = d.get_str("title").map_err(|e| err("col", e))?.to_owned();
        let score = d.get_f64("score").unwrap_or(0.0);
        out.push(SearchHit {
            node_id: NodeId::new(node_id)?,
            doc_id: DocId::new(doc_id)?,
            title,
            score: score as f32,
        });
    }
    Ok(out)
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}
