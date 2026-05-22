//! The [`StorageAdapter`] trait every backend implements.
//!
//! Adapters own the persistence of nodes, raw text, summaries, and document
//! metadata. The pagebridge library coordinates across adapters but never
//! holds the storage itself. This is what makes the same engine work against
//! Postgres, SQLite, MongoDB, an embedded redb+tantivy store, or plain JSON.

use async_trait::async_trait;

use crate::error::Result;
use crate::id::{DocId, NodeId};
use crate::record::{NodeRecord, NodeSummary};
use crate::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};

/// The single contract every storage backend satisfies.
///
/// Adapter implementations must be cheap to clone (typically via `Arc<...>`),
/// thread-safe, and async-safe. All methods are designed to be called
/// concurrently across many tokio tasks.
#[async_trait]
pub trait StorageAdapter: Send + Sync + 'static {
    /// Human-readable name of this adapter, e.g. "postgres", "sqlite", "mongodb".
    fn name(&self) -> &'static str;

    /// Set up the underlying storage (create tables, indexes, etc). Idempotent.
    async fn migrate(&self) -> Result<()>;

    /// Upsert a single node record. Idempotent on `node_id`. Used for both the
    /// structural insert (empty summaries) and the later summary update.
    async fn upsert_node(&self, node: &NodeRecord) -> Result<()>;

    /// Batch upsert. Override for efficiency; default delegates to `upsert_node`.
    async fn upsert_nodes(&self, nodes: &[NodeRecord]) -> Result<()> {
        for n in nodes {
            self.upsert_node(n).await?;
        }
        Ok(())
    }

    /// Fetch a single node by id. Returns `Ok(None)` if absent.
    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>>;

    /// Batch get. Returned vector is in the same order as requested. Missing ids
    /// are silently skipped (the caller can detect this via length mismatch).
    async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodeRecord>>;

    /// Children of a node, projected to the lightweight [`NodeSummary`] for cheap
    /// loading during navigation. MUST be fast (sub-50ms p99 in production).
    async fn children_summaries(&self, parent: &NodeId) -> Result<Vec<NodeSummary>>;

    /// Children of a node as full records. Used during ingest and synthesis.
    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>>;

    /// Walk from the document root down to `id`, inclusive.
    async fn path_to(&self, id: &NodeId) -> Result<Vec<NodeRecord>>;

    /// Collect every leaf id under `root` (inclusive of leaves equal to `root`
    /// when `root` is itself a leaf).
    async fn leaves_under(&self, root: &NodeId) -> Result<Vec<NodeId>>;

    /// Remove a document and all its nodes, raw text, and trace data.
    async fn delete_document(&self, doc_id: &DocId) -> Result<()>;

    /// List every document this adapter holds.
    async fn list_documents(&self) -> Result<Vec<DocumentEntry>>;

    /// Persist (or update) a document entry.
    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()>;

    /// BM25 (or the backend's native FTS equivalent) over all leaves. Scores are
    /// always returned with the convention "higher is more relevant".
    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>>;

    /// Same as `bm25_search`, scoped to a single document.
    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>>;

    /// Append raw bytes for a document. Returns the starting offset of the data
    /// just written.
    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64>;

    /// Read a byte span. Adapters MUST return exactly `span.1 - span.0` bytes
    /// or an error.
    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>>;

    /// Read a byte span as UTF-8 text. Implementations may use `read_raw_span`
    /// and decode, or use the backend's native string accessor.
    async fn read_raw_text(&self, doc_id: &DocId, span: (u64, u64)) -> Result<String> {
        let bytes = self.read_raw_span(doc_id, span).await?;
        String::from_utf8(bytes).map_err(|e| crate::error::PagebridgeError::Parse {
            source_kind: "utf8".into(),
            message: e.to_string(),
        })
    }

    /// Look up a cached summary by source content hash.
    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>>;

    /// Store a cached summary.
    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()>;

    /// Adapter-side counters. Best-effort: implementations may return approximate
    /// counts for large stores.
    async fn stats(&self) -> Result<AdapterStats>;

    /// Cheap connectivity check.
    async fn ping(&self) -> Result<()>;
}

/// In-memory adapter for unit-testing higher-level logic.
///
/// Not for production: BM25 is approximated by token overlap and the store is
/// not persistent.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::assigning_clones,
    clippy::significant_drop_tightening
)]
pub mod memory {
    use super::StorageAdapter;
    use crate::error::PagebridgeError;
    use crate::error::Result;
    use crate::id::{DocId, NodeId};
    use crate::record::{NodeRecord, NodeSummary};
    use crate::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
    use async_trait::async_trait;
    use dashmap::DashMap;
    use parking_lot::RwLock;
    use std::collections::BTreeMap;

    /// In-memory adapter. Intentionally simplistic: BM25 is approximated by token
    /// overlap, raw blobs live in a per-doc `Vec<u8>`.
    #[derive(Debug, Default)]
    pub struct MemoryAdapter {
        nodes: DashMap<NodeId, NodeRecord>,
        docs: DashMap<DocId, DocumentEntry>,
        raw: RwLock<BTreeMap<DocId, Vec<u8>>>,
        summary_cache: DashMap<[u8; 32], SummaryCacheEntry>,
    }

    impl MemoryAdapter {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl StorageAdapter for MemoryAdapter {
        fn name(&self) -> &'static str {
            "memory"
        }

        async fn migrate(&self) -> Result<()> {
            Ok(())
        }

        async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
            node.validate()?;
            self.nodes.insert(node.node_id.clone(), node.clone());
            Ok(())
        }

        async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
            Ok(self.nodes.get(id).map(|r| r.value().clone()))
        }

        async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodeRecord>> {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(r) = self.nodes.get(id) {
                    out.push(r.value().clone());
                }
            }
            Ok(out)
        }

        async fn children_summaries(&self, parent: &NodeId) -> Result<Vec<NodeSummary>> {
            let mut out: Vec<NodeSummary> = self
                .nodes
                .iter()
                .filter(|r| r.parent_id.as_ref() == Some(parent))
                .map(|r| NodeSummary::from(r.value()))
                .collect();
            out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
            Ok(out)
        }

        async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
            let mut out: Vec<NodeRecord> = self
                .nodes
                .iter()
                .filter(|r| r.parent_id.as_ref() == Some(parent))
                .map(|r| r.value().clone())
                .collect();
            out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
            Ok(out)
        }

        async fn path_to(&self, id: &NodeId) -> Result<Vec<NodeRecord>> {
            let mut chain = Vec::new();
            let mut cursor = Some(id.clone());
            while let Some(c) = cursor {
                let Some(r) = self.nodes.get(&c) else { break };
                let rec = r.value().clone();
                cursor = rec.parent_id.clone();
                chain.push(rec);
            }
            chain.reverse();
            Ok(chain)
        }

        async fn leaves_under(&self, root: &NodeId) -> Result<Vec<NodeId>> {
            let mut out: Vec<NodeId> = self
                .nodes
                .iter()
                .filter(|r| r.is_leaf && root.is_prefix_of(&r.node_id))
                .map(|r| r.node_id.clone())
                .collect();
            out.sort();
            Ok(out)
        }

        async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
            self.nodes.retain(|_, v| v.doc_id != *doc_id);
            self.docs.remove(doc_id);
            self.raw.write().remove(doc_id);
            Ok(())
        }

        async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
            let mut out: Vec<DocumentEntry> = self.docs.iter().map(|r| r.value().clone()).collect();
            out.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
            Ok(out)
        }

        async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
            self.docs.insert(doc.doc_id.clone(), doc.clone());
            Ok(())
        }

        async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
            let q_terms = tokenize(query);
            let mut hits: Vec<SearchHit> = self
                .nodes
                .iter()
                .filter(|r| r.is_leaf)
                .filter_map(|r| {
                    let haystack = format!(
                        "{} {} {} {}",
                        r.title,
                        r.routing_summary,
                        r.summary,
                        r.keywords.join(" ")
                    );
                    let score = score_overlap(&q_terms, &tokenize(&haystack));
                    if score > 0.0 {
                        Some(SearchHit {
                            node_id: r.node_id.clone(),
                            doc_id: r.doc_id.clone(),
                            title: r.title.clone(),
                            score,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.truncate(limit);
            Ok(hits)
        }

        async fn bm25_search_in_doc(
            &self,
            doc_id: &DocId,
            query: &str,
            limit: usize,
        ) -> Result<Vec<SearchHit>> {
            let mut hits = self.bm25_search(query, limit * 4).await?;
            hits.retain(|h| h.doc_id == *doc_id);
            hits.truncate(limit);
            Ok(hits)
        }

        async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
            let mut raw = self.raw.write();
            let buf = raw.entry(doc_id.clone()).or_default();
            let offset = buf.len() as u64;
            buf.extend_from_slice(data);
            Ok(offset)
        }

        async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
            let raw = self.raw.read();
            let buf = raw
                .get(doc_id)
                .ok_or_else(|| PagebridgeError::DocumentNotFound(doc_id.clone()))?;
            let (a, b) = (span.0 as usize, span.1 as usize);
            if b > buf.len() || a > b {
                return Err(PagebridgeError::InvalidArgument(format!(
                    "span {span:?} out of bounds (len={})",
                    buf.len()
                )));
            }
            Ok(buf[a..b].to_vec())
        }

        async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
            Ok(self.summary_cache.get(hash).map(|r| r.value().clone()))
        }

        async fn upsert_summary_cache(
            &self,
            hash: &[u8; 32],
            entry: &SummaryCacheEntry,
        ) -> Result<()> {
            self.summary_cache.insert(*hash, entry.clone());
            Ok(())
        }

        async fn stats(&self) -> Result<AdapterStats> {
            Ok(AdapterStats {
                node_count: self.nodes.len() as u64,
                document_count: self.docs.len() as u64,
                raw_bytes: self.raw.read().values().map(|v| v.len() as u64).sum(),
                summary_cache_entries: self.summary_cache.len() as u64,
            })
        }

        async fn ping(&self) -> Result<()> {
            Ok(())
        }
    }

    fn tokenize(s: &str) -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn score_overlap(query: &[String], haystack: &[String]) -> f32 {
        if query.is_empty() {
            return 0.0;
        }
        let hay: std::collections::HashSet<&str> = haystack.iter().map(String::as_str).collect();
        let matched = query.iter().filter(|t| hay.contains(t.as_str())).count();
        matched as f32 / query.len() as f32
    }
}

pub use memory::MemoryAdapter;
