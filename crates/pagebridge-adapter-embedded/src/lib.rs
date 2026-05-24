//! Embedded storage adapter for pagebridge.
//!
//! Stores everything under a single directory:
//! - `pagebridge.redb` for nodes, document index, raw offsets, summary cache.
//! - `pagebridge.tantivy/` for the BM25 inverted index.
//! - `raw/<doc_id>.bin` for the raw bytes of each ingested document.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::significant_drop_tightening,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use parking_lot::Mutex;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const NODES: TableDefinition<&str, Vec<u8>> = TableDefinition::new("nodes");
const DOCS: TableDefinition<&str, Vec<u8>> = TableDefinition::new("docs");
const SUMMARY_CACHE: TableDefinition<[u8; 32], Vec<u8>> = TableDefinition::new("summary_cache");
const RAW_OFFSETS: TableDefinition<&str, u64> = TableDefinition::new("raw_offsets");

struct TantivyFields {
    node_id: Field,
    doc_id: Field,
    title: Field,
    routing_summary: Field,
    summary: Field,
    keywords: Field,
    level: Field,
}

/// Embedded (redb + tantivy) storage adapter. Cheap to clone via `Arc`.
pub struct EmbeddedAdapter {
    root: PathBuf,
    db: Arc<Database>,
    index: Index,
    fields: TantivyFields,
    reader: IndexReader,
    writer: Arc<Mutex<IndexWriter>>,
}

impl std::fmt::Debug for EmbeddedAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedAdapter")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl EmbeddedAdapter {
    /// Open or create an embedded store rooted at `path`.
    ///
    /// The directory is created if it does not exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join("raw"))?;

        let db_path = root.join("pagebridge.redb");
        let db = Database::create(&db_path).map_err(|e| PagebridgeError::Adapter {
            adapter: "embedded".into(),
            message: format!("redb open: {e}"),
        })?;

        let (index, fields) = open_or_create_index(&root)?;
        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| PagebridgeError::Adapter {
                adapter: "embedded".into(),
                message: format!("tantivy reader: {e}"),
            })?;
        let writer = index
            .writer(64 * 1024 * 1024)
            .map_err(|e| PagebridgeError::Adapter {
                adapter: "embedded".into(),
                message: format!("tantivy writer: {e}"),
            })?;
        Ok(Self {
            root,
            db: Arc::new(db),
            index,
            fields,
            reader,
            writer: Arc::new(Mutex::new(writer)),
        })
    }

    fn raw_path(&self, doc_id: &DocId) -> PathBuf {
        self.root.join("raw").join(format!("{doc_id}.bin"))
    }

    fn upsert_node_sync(&self, node: &NodeRecord) -> Result<()> {
        node.validate()?;
        let encoded = bincode::serialize(node)
            .map_err(|e| PagebridgeError::Serde(format!("encode node: {e}")))?;
        let tx = self
            .db
            .begin_write()
            .map_err(|e| adapter_err("begin write", e))?;
        {
            let mut tbl = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("open nodes", e))?;
            tbl.insert(node.node_id.as_str(), encoded)
                .map_err(|e| adapter_err("insert node", e))?;
        }
        tx.commit().map_err(|e| adapter_err("commit", e))?;

        // Update the FTS index.
        let mut writer = self.writer.lock();
        writer.delete_term(Term::from_field_text(
            self.fields.node_id,
            node.node_id.as_str(),
        ));
        writer
            .add_document(self.build_doc(node))
            .map_err(|e| adapter_err("tantivy add", e))?;
        writer
            .commit()
            .map_err(|e| adapter_err("tantivy commit", e))?;
        drop(writer);
        self.reader
            .reload()
            .map_err(|e| adapter_err("reader reload", e))?;
        Ok(())
    }

    fn upsert_nodes_batch_sync(&self, nodes: &[NodeRecord]) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }
        for n in nodes {
            n.validate()?;
        }
        // One redb write transaction for the whole batch.
        let tx = self
            .db
            .begin_write()
            .map_err(|e| adapter_err("batch begin write", e))?;
        {
            let mut tbl = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("batch open nodes", e))?;
            for node in nodes {
                let encoded = bincode::serialize(node)
                    .map_err(|e| PagebridgeError::Serde(format!("encode node: {e}")))?;
                tbl.insert(node.node_id.as_str(), encoded)
                    .map_err(|e| adapter_err("batch insert node", e))?;
            }
        }
        tx.commit().map_err(|e| adapter_err("batch commit", e))?;

        // One tantivy commit at the end so per-node segment flushes don't
        // dominate the wall time. Lazy commit cadence is owned by Phase I4;
        // here we just collapse N commits into one.
        let mut writer = self.writer.lock();
        for node in nodes {
            writer.delete_term(Term::from_field_text(
                self.fields.node_id,
                node.node_id.as_str(),
            ));
            writer
                .add_document(self.build_doc(node))
                .map_err(|e| adapter_err("batch tantivy add", e))?;
        }
        writer
            .commit()
            .map_err(|e| adapter_err("batch tantivy commit", e))?;
        drop(writer);
        self.reader
            .reload()
            .map_err(|e| adapter_err("reader reload", e))?;
        Ok(())
    }

    fn build_doc(&self, node: &NodeRecord) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.node_id, node.node_id.as_str());
        doc.add_text(self.fields.doc_id, node.doc_id.as_str());
        doc.add_text(self.fields.title, &node.title);
        doc.add_text(self.fields.routing_summary, &node.routing_summary);
        doc.add_text(self.fields.summary, &node.summary);
        for kw in &node.keywords {
            doc.add_text(self.fields.keywords, kw);
        }
        let lvl: u64 = match node.level {
            pagebridge_core::record::NodeLevel::Corpus => 0,
            pagebridge_core::record::NodeLevel::Document => 1,
            pagebridge_core::record::NodeLevel::Section => 2,
            pagebridge_core::record::NodeLevel::Subsection => 3,
            pagebridge_core::record::NodeLevel::Page => 4,
            pagebridge_core::record::NodeLevel::Leaf => 5,
        };
        doc.add_u64(self.fields.level, lvl);
        doc
    }

    fn get_node_sync(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| adapter_err("begin read", e))?;
        let tbl = tx
            .open_table(NODES)
            .map_err(|e| adapter_err("open nodes", e))?;
        let Some(g) = tbl
            .get(id.as_str())
            .map_err(|e| adapter_err("get node", e))?
        else {
            return Ok(None);
        };
        let rec: NodeRecord = bincode::deserialize(g.value().as_slice())
            .map_err(|e| PagebridgeError::Serde(format!("decode node: {e}")))?;
        Ok(Some(rec))
    }

    fn search_sync(
        &self,
        query: &str,
        limit: usize,
        doc_filter: Option<&DocId>,
    ) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();
        let parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.title,
                self.fields.routing_summary,
                self.fields.summary,
                self.fields.keywords,
            ],
        );
        let parsed = parser
            .parse_query(query)
            .map_err(|e| adapter_err("parse query", e))?;
        let take = if doc_filter.is_some() {
            limit.saturating_mul(8).max(limit)
        } else {
            limit
        };
        let top = searcher
            .search(&parsed, &TopDocs::with_limit(take.max(1)))
            .map_err(|e| adapter_err("search", e))?;
        let mut out = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher
                .doc(addr)
                .map_err(|e| adapter_err("fetch doc", e))?;
            let Some(node_id_v) = doc
                .get_first(self.fields.node_id)
                .and_then(|v| v.as_str().map(str::to_owned))
            else {
                continue;
            };
            let Some(doc_id_v) = doc
                .get_first(self.fields.doc_id)
                .and_then(|v| v.as_str().map(str::to_owned))
            else {
                continue;
            };
            let title = doc
                .get_first(self.fields.title)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let node_id = NodeId::new(node_id_v)?;
            let doc_id_p = DocId::new(doc_id_v)?;
            if let Some(filter) = doc_filter {
                if doc_id_p != *filter {
                    continue;
                }
            }
            out.push(SearchHit {
                node_id,
                doc_id: doc_id_p,
                title,
                score,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}

fn open_or_create_index(root: &Path) -> Result<(Index, TantivyFields)> {
    let dir = root.join("pagebridge.tantivy");
    std::fs::create_dir_all(&dir)?;
    let mut schema = Schema::builder();
    let node_id = schema.add_text_field("node_id", STRING | STORED);
    let doc_id = schema.add_text_field("doc_id", STRING | STORED);
    let title = schema.add_text_field("title", TEXT | STORED);
    let routing_summary = schema.add_text_field("routing_summary", TEXT);
    let summary = schema.add_text_field("summary", TEXT);
    let keywords = schema.add_text_field("keywords", TEXT);
    let level = schema.add_u64_field("level", STORED);
    let schema = schema.build();
    let index = if Index::exists(
        &tantivy::directory::MmapDirectory::open(&dir).map_err(|e| adapter_err("mmap dir", e))?,
    )
    .map_err(|e| adapter_err("index exists", e))?
    {
        Index::open_in_dir(&dir).map_err(|e| adapter_err("open index", e))?
    } else {
        Index::create_in_dir(&dir, schema).map_err(|e| adapter_err("create index", e))?
    };
    let fields = TantivyFields {
        node_id,
        doc_id,
        title,
        routing_summary,
        summary,
        keywords,
        level,
    };
    Ok((index, fields))
}

fn adapter_err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "embedded".into(),
        message: format!("{ctx}: {e}"),
    }
}

#[async_trait]
impl StorageAdapter for EmbeddedAdapter {
    fn name(&self) -> &'static str {
        "embedded"
    }

    async fn migrate(&self) -> Result<()> {
        // Touching every table from a write transaction creates them if missing.
        let tx = self
            .db
            .begin_write()
            .map_err(|e| adapter_err("migrate write", e))?;
        {
            let _ = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("ensure nodes", e))?;
            let _ = tx
                .open_table(DOCS)
                .map_err(|e| adapter_err("ensure docs", e))?;
            let _ = tx
                .open_table(SUMMARY_CACHE)
                .map_err(|e| adapter_err("ensure cache", e))?;
            let _ = tx
                .open_table(RAW_OFFSETS)
                .map_err(|e| adapter_err("ensure offsets", e))?;
        }
        tx.commit().map_err(|e| adapter_err("migrate commit", e))?;
        Ok(())
    }

    async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
        let me = self.clone_handle();
        let node = node.clone();
        tokio::task::spawn_blocking(move || me.upsert_node_sync(&node))
            .await
            .map_err(|e| adapter_err("join", e))?
    }

    async fn upsert_nodes_batch(&self, nodes: &[NodeRecord]) -> Result<()> {
        let me = self.clone_handle();
        let nodes = nodes.to_vec();
        tokio::task::spawn_blocking(move || me.upsert_nodes_batch_sync(&nodes))
            .await
            .map_err(|e| adapter_err("batch join", e))?
    }

    fn recommended_batch_size(&self) -> usize {
        1_000
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let me = self.clone_handle();
        let id = id.clone();
        tokio::task::spawn_blocking(move || me.get_node_sync(&id))
            .await
            .map_err(|e| adapter_err("join", e))?
    }

    async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodeRecord>> {
        let me = self.clone_handle();
        let ids = ids.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                if let Some(n) = me.get_node_sync(id)? {
                    out.push(n);
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn children_summaries(&self, parent: &NodeId) -> Result<Vec<NodeSummary>> {
        let recs = self.children_records(parent).await?;
        Ok(recs.iter().map(NodeSummary::from).collect())
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        let me = self.clone_handle();
        let parent = parent.clone();
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_read()
                .map_err(|e| adapter_err("begin read", e))?;
            let tbl = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("open nodes", e))?;
            let prefix = format!("{}/", parent.as_str());
            let range = tbl
                .range(prefix.as_str()..)
                .map_err(|e| adapter_err("range", e))?;
            let mut out = Vec::new();
            for entry in range {
                let (k, v) = entry.map_err(|e| adapter_err("range entry", e))?;
                let key = k.value();
                if !key.starts_with(&prefix) {
                    break;
                }
                // Only direct children, not deeper descendants.
                let suffix = &key[prefix.len()..];
                if suffix.contains('/') {
                    continue;
                }
                let rec: NodeRecord = bincode::deserialize(v.value().as_slice())
                    .map_err(|e| PagebridgeError::Serde(format!("decode node: {e}")))?;
                out.push(rec);
            }
            out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
            Ok(out)
        })
        .await
        .map_err(|e| adapter_err("join", e))?
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
        let me = self.clone_handle();
        let root = root.clone();
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_read()
                .map_err(|e| adapter_err("begin read", e))?;
            let tbl = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("open nodes", e))?;
            let mut out = Vec::new();
            let start = root.as_str().to_owned();
            let range = tbl
                .range(start.as_str()..)
                .map_err(|e| adapter_err("range", e))?;
            for entry in range {
                let (k, v) = entry.map_err(|e| adapter_err("range entry", e))?;
                let key = k.value().to_owned();
                if key != root.as_str() && !key.starts_with(&format!("{start}/")) {
                    break;
                }
                let rec: NodeRecord = bincode::deserialize(v.value().as_slice())
                    .map_err(|e| PagebridgeError::Serde(format!("decode node: {e}")))?;
                if rec.is_leaf {
                    out.push(rec.node_id);
                }
            }
            out.sort();
            Ok(out)
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
        let me = self.clone_handle();
        let doc_id = doc_id.clone();
        let path = self.raw_path(&doc_id);
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_write()
                .map_err(|e| adapter_err("begin write", e))?;
            let mut to_delete = Vec::new();
            {
                let tbl = tx
                    .open_table(NODES)
                    .map_err(|e| adapter_err("open nodes", e))?;
                let prefix = format!("doc:{doc_id}");
                for entry in tbl
                    .range(prefix.as_str()..)
                    .map_err(|e| adapter_err("range", e))?
                {
                    let (k, _) = entry.map_err(|e| adapter_err("range entry", e))?;
                    let key = k.value().to_owned();
                    if key != prefix && !key.starts_with(&format!("{prefix}/")) {
                        break;
                    }
                    to_delete.push(key);
                }
            }
            {
                let mut tbl = tx
                    .open_table(NODES)
                    .map_err(|e| adapter_err("open nodes", e))?;
                for k in &to_delete {
                    tbl.remove(k.as_str())
                        .map_err(|e| adapter_err("remove node", e))?;
                }
            }
            {
                let mut tbl = tx
                    .open_table(DOCS)
                    .map_err(|e| adapter_err("open docs", e))?;
                tbl.remove(doc_id.as_str())
                    .map_err(|e| adapter_err("remove doc", e))?;
            }
            {
                let mut tbl = tx
                    .open_table(RAW_OFFSETS)
                    .map_err(|e| adapter_err("open offsets", e))?;
                tbl.remove(doc_id.as_str())
                    .map_err(|e| adapter_err("remove offset", e))?;
            }
            tx.commit().map_err(|e| adapter_err("commit", e))?;

            // Tantivy.
            let mut writer = me.writer.lock();
            for k in &to_delete {
                writer.delete_term(Term::from_field_text(me.fields.node_id, k));
            }
            writer
                .commit()
                .map_err(|e| adapter_err("tantivy commit", e))?;
            drop(writer);
            me.reader
                .reload()
                .map_err(|e| adapter_err("reader reload", e))?;

            // Remove raw file.
            let _ = std::fs::remove_file(&path);
            Ok(())
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        let me = self.clone_handle();
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_read()
                .map_err(|e| adapter_err("begin read", e))?;
            let tbl = tx
                .open_table(DOCS)
                .map_err(|e| adapter_err("open docs", e))?;
            let mut out = Vec::new();
            for entry in tbl.iter().map_err(|e| adapter_err("iter docs", e))? {
                let (_, v) = entry.map_err(|e| adapter_err("iter entry", e))?;
                let de: DocumentEntry = bincode::deserialize(v.value().as_slice())
                    .map_err(|e| PagebridgeError::Serde(format!("decode doc: {e}")))?;
                out.push(de);
            }
            out.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
            Ok(out)
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        let me = self.clone_handle();
        let doc = doc.clone();
        tokio::task::spawn_blocking(move || {
            let encoded = bincode::serialize(&doc)
                .map_err(|e| PagebridgeError::Serde(format!("encode doc: {e}")))?;
            let tx = me
                .db
                .begin_write()
                .map_err(|e| adapter_err("begin write", e))?;
            {
                let mut tbl = tx
                    .open_table(DOCS)
                    .map_err(|e| adapter_err("open docs", e))?;
                tbl.insert(doc.doc_id.as_str(), encoded)
                    .map_err(|e| adapter_err("insert doc", e))?;
            }
            tx.commit().map_err(|e| adapter_err("commit", e))?;
            Ok(())
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let me = self.clone_handle();
        let query = query.to_owned();
        tokio::task::spawn_blocking(move || me.search_sync(&query, limit, None))
            .await
            .map_err(|e| adapter_err("join", e))?
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let me = self.clone_handle();
        let query = query.to_owned();
        let doc_id = doc_id.clone();
        tokio::task::spawn_blocking(move || me.search_sync(&query, limit, Some(&doc_id)))
            .await
            .map_err(|e| adapter_err("join", e))?
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        let path = self.raw_path(doc_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let offset = file.metadata().await?.len();
        file.write_all(data).await?;
        file.flush().await?;

        let me = self.clone_handle();
        let doc_id_s = doc_id.as_str().to_owned();
        let new_offset = offset + data.len() as u64;
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_write()
                .map_err(|e| adapter_err("begin write", e))?;
            {
                let mut tbl = tx
                    .open_table(RAW_OFFSETS)
                    .map_err(|e| adapter_err("open offsets", e))?;
                tbl.insert(doc_id_s.as_str(), new_offset)
                    .map_err(|e| adapter_err("insert offset", e))?;
            }
            tx.commit().map_err(|e| adapter_err("commit", e))?;
            Ok::<(), PagebridgeError>(())
        })
        .await
        .map_err(|e| adapter_err("join", e))??;
        Ok(offset)
    }

    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
        if span.0 > span.1 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "read_raw_span: start {} > end {}",
                span.0, span.1
            )));
        }
        let path = self.raw_path(doc_id);
        let mut file = tokio::fs::File::open(&path)
            .await
            .map_err(|_| PagebridgeError::DocumentNotFound(doc_id.clone()))?;
        file.seek(std::io::SeekFrom::Start(span.0)).await?;
        let len = (span.1 - span.0) as usize;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf).await?;
        Ok(buf)
    }

    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        let me = self.clone_handle();
        let hash = *hash;
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_read()
                .map_err(|e| adapter_err("begin read", e))?;
            let tbl = tx
                .open_table(SUMMARY_CACHE)
                .map_err(|e| adapter_err("open cache", e))?;
            let Some(g) = tbl.get(hash).map_err(|e| adapter_err("get cache", e))? else {
                return Ok(None);
            };
            let v: SummaryCacheEntry = bincode::deserialize(g.value().as_slice())
                .map_err(|e| PagebridgeError::Serde(format!("decode cache: {e}")))?;
            Ok(Some(v))
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        let me = self.clone_handle();
        let hash = *hash;
        let entry = entry.clone();
        tokio::task::spawn_blocking(move || {
            let encoded = bincode::serialize(&entry)
                .map_err(|e| PagebridgeError::Serde(format!("encode cache: {e}")))?;
            let tx = me
                .db
                .begin_write()
                .map_err(|e| adapter_err("begin write", e))?;
            {
                let mut tbl = tx
                    .open_table(SUMMARY_CACHE)
                    .map_err(|e| adapter_err("open cache", e))?;
                tbl.insert(hash, encoded)
                    .map_err(|e| adapter_err("insert cache", e))?;
            }
            tx.commit().map_err(|e| adapter_err("commit", e))?;
            Ok(())
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn stats(&self) -> Result<AdapterStats> {
        let me = self.clone_handle();
        let raw_dir = self.root.join("raw");
        tokio::task::spawn_blocking(move || {
            let tx = me
                .db
                .begin_read()
                .map_err(|e| adapter_err("begin read", e))?;
            let nodes = tx
                .open_table(NODES)
                .map_err(|e| adapter_err("open nodes", e))?;
            let docs = tx
                .open_table(DOCS)
                .map_err(|e| adapter_err("open docs", e))?;
            let cache = tx
                .open_table(SUMMARY_CACHE)
                .map_err(|e| adapter_err("open cache", e))?;
            let mut raw_bytes = 0u64;
            if let Ok(read_dir) = std::fs::read_dir(&raw_dir) {
                for entry in read_dir.flatten() {
                    if let Ok(md) = entry.metadata() {
                        raw_bytes += md.len();
                    }
                }
            }
            Ok(AdapterStats {
                node_count: nodes.len().unwrap_or(0),
                document_count: docs.len().unwrap_or(0),
                raw_bytes,
                summary_cache_entries: cache.len().unwrap_or(0),
            })
        })
        .await
        .map_err(|e| adapter_err("join", e))?
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }
}

// Hand-rolled clone helper since Database/IndexReader are wrapped in Arc.
impl EmbeddedAdapter {
    fn clone_handle(&self) -> Self {
        Self {
            root: self.root.clone(),
            db: Arc::clone(&self.db),
            index: self.index.clone(),
            fields: TantivyFields {
                node_id: self.fields.node_id,
                doc_id: self.fields.doc_id,
                title: self.fields.title,
                routing_summary: self.fields.routing_summary,
                summary: self.fields.summary,
                keywords: self.fields.keywords,
                level: self.fields.level,
            },
            reader: self.reader.clone(),
            writer: Arc::clone(&self.writer),
        }
    }
}
