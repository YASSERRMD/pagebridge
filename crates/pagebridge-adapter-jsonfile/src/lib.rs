//! JSON-file prototyping storage adapter for pagebridge.
//!
//! Stores one JSON file per document plus a global index file. Raw text lives
//! in plain `.bin` files. There is no real BM25: a substring scoring scheme is
//! used as a fallback. For production retrieval use one of the SQL-backed or
//! the embedded redb+tantivy adapter.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::unused_async,
    clippy::significant_drop_tightening,
    clippy::significant_drop_in_scrutinee,
    clippy::needless_collect,
    clippy::needless_pass_by_value,
    clippy::assigning_clones,
    clippy::option_if_let_else
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::error::{PagebridgeError, Result};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeRecord, NodeSummary};
use pagebridge_core::types::{AdapterStats, DocumentEntry, SearchHit, SummaryCacheEntry};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct DocTree {
    entry: Option<DocumentEntry>,
    nodes: HashMap<String, NodeRecord>,
}

/// Directory-based storage adapter. One JSON file per document plus an index.
#[derive(Clone)]
pub struct JsonFileAdapter {
    root: PathBuf,
    cache: Arc<RwLock<HashMap<DocId, DocTree>>>,
    summaries: Arc<RwLock<HashMap<[u8; 32], SummaryCacheEntry>>>,
}

impl std::fmt::Debug for JsonFileAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileAdapter")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl JsonFileAdapter {
    /// Open or create a directory-based store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join("trees"))?;
        std::fs::create_dir_all(root.join("raw"))?;
        let summaries_path = root.join("summaries.json");
        let summaries: HashMap<[u8; 32], SummaryCacheEntry> = if summaries_path.exists() {
            let bytes = std::fs::read(&summaries_path)?;
            // Serialize hashes as hex to keep JSON valid.
            let map: HashMap<String, SummaryCacheEntry> =
                serde_json::from_slice(&bytes).unwrap_or_default();
            map.into_iter()
                .filter_map(|(k, v)| hex_to_hash(&k).map(|h| (h, v)))
                .collect()
        } else {
            HashMap::new()
        };
        Ok(Self {
            root,
            cache: Arc::new(RwLock::new(HashMap::new())),
            summaries: Arc::new(RwLock::new(summaries)),
        })
    }

    fn tree_path(&self, doc: &DocId) -> PathBuf {
        self.root.join("trees").join(format!("{doc}.json"))
    }
    fn raw_path(&self, doc: &DocId) -> PathBuf {
        self.root.join("raw").join(format!("{doc}.bin"))
    }
    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }
    fn summaries_path(&self) -> PathBuf {
        self.root.join("summaries.json")
    }

    fn load_tree(&self, doc: &DocId) -> Result<DocTree> {
        {
            if let Some(t) = self.cache.read().get(doc) {
                return Ok(clone_tree(t));
            }
        }
        let p = self.tree_path(doc);
        if !p.exists() {
            let mut cache = self.cache.write();
            cache.insert(doc.clone(), DocTree::default());
            return Ok(DocTree::default());
        }
        let bytes = std::fs::read(&p)?;
        let tree: DocTree = serde_json::from_slice(&bytes).map_err(|e| err("decode tree", e))?;
        let copy = clone_tree(&tree);
        self.cache.write().insert(doc.clone(), tree);
        Ok(copy)
    }

    fn save_tree(&self, doc: &DocId, tree: &DocTree) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(tree).map_err(|e| err("encode tree", e))?;
        let final_path = self.tree_path(doc);
        let tmp = final_path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &final_path)?;
        Ok(())
    }

    fn save_index(&self) -> Result<()> {
        let cache = self.cache.read();
        let entries: Vec<DocumentEntry> = cache.values().filter_map(|t| t.entry.clone()).collect();
        let bytes = serde_json::to_vec_pretty(&entries).map_err(|e| err("encode index", e))?;
        let final_path = self.index_path();
        let tmp = final_path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &final_path)?;
        Ok(())
    }

    fn save_summaries(&self) -> Result<()> {
        let map = self.summaries.read();
        let serial: HashMap<String, &SummaryCacheEntry> =
            map.iter().map(|(k, v)| (hash_to_hex(k), v)).collect();
        let bytes = serde_json::to_vec_pretty(&serial).map_err(|e| err("encode sums", e))?;
        let tmp = self.summaries_path().with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, self.summaries_path())?;
        Ok(())
    }
}

fn clone_tree(t: &DocTree) -> DocTree {
    DocTree {
        entry: t.entry.clone(),
        nodes: t.nodes.clone(),
    }
}

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> PagebridgeError {
    PagebridgeError::Adapter {
        adapter: "jsonfile".into(),
        message: format!("{ctx}: {e}"),
    }
}

fn hash_to_hex(h: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in h {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn hex_to_hash(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

fn score_overlap(query: &[String], hay: &[String]) -> f32 {
    if query.is_empty() {
        return 0.0;
    }
    let set: std::collections::HashSet<&str> = hay.iter().map(String::as_str).collect();
    let matched = query.iter().filter(|t| set.contains(t.as_str())).count();
    matched as f32 / query.len() as f32
}

#[async_trait]
impl StorageAdapter for JsonFileAdapter {
    fn name(&self) -> &'static str {
        "jsonfile"
    }

    async fn migrate(&self) -> Result<()> {
        Ok(())
    }

    async fn upsert_node(&self, node: &NodeRecord) -> Result<()> {
        node.validate()?;
        let me = self.clone();
        let node = node.clone();
        tokio::task::spawn_blocking(move || {
            let mut tree = me.load_tree(&node.doc_id)?;
            tree.nodes
                .insert(node.node_id.as_str().to_owned(), node.clone());
            me.cache
                .write()
                .insert(node.doc_id.clone(), clone_tree(&tree));
            me.save_tree(&node.doc_id, &tree)?;
            Ok(())
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn get_node(&self, id: &NodeId) -> Result<Option<NodeRecord>> {
        let doc = id.doc_id()?;
        let tree = self.load_tree(&doc)?;
        Ok(tree.nodes.get(id.as_str()).cloned())
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
        let doc = parent.doc_id()?;
        let tree = self.load_tree(&doc)?;
        let mut out: Vec<NodeSummary> = tree
            .nodes
            .values()
            .filter(|n| n.parent_id.as_ref() == Some(parent))
            .map(NodeSummary::from)
            .collect();
        out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Ok(out)
    }

    async fn children_records(&self, parent: &NodeId) -> Result<Vec<NodeRecord>> {
        let doc = parent.doc_id()?;
        let tree = self.load_tree(&doc)?;
        let mut out: Vec<NodeRecord> = tree
            .nodes
            .values()
            .filter(|n| n.parent_id.as_ref() == Some(parent))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
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
        let doc = root.doc_id()?;
        let tree = self.load_tree(&doc)?;
        let mut out: Vec<NodeId> = tree
            .nodes
            .values()
            .filter(|n| n.is_leaf && root.is_prefix_of(&n.node_id))
            .map(|n| n.node_id.clone())
            .collect();
        out.sort();
        Ok(out)
    }

    async fn delete_document(&self, doc_id: &DocId) -> Result<()> {
        let me = self.clone();
        let doc_id = doc_id.clone();
        tokio::task::spawn_blocking(move || {
            me.cache.write().remove(&doc_id);
            let _ = std::fs::remove_file(me.tree_path(&doc_id));
            let _ = std::fs::remove_file(me.raw_path(&doc_id));
            me.save_index()?;
            Ok(())
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        // Eagerly load every tree.json under trees/.
        let me = self.clone();
        let trees_dir = self.root.join("trees");
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            if let Ok(read_dir) = std::fs::read_dir(&trees_dir) {
                for entry in read_dir.flatten() {
                    if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                        if let Ok(doc) = DocId::new(stem) {
                            let tree = me.load_tree(&doc)?;
                            if let Some(de) = tree.entry {
                                out.push(de);
                            }
                        }
                    }
                }
            }
            out.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
            Ok(out)
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn upsert_document(&self, doc: &DocumentEntry) -> Result<()> {
        let me = self.clone();
        let doc = doc.clone();
        tokio::task::spawn_blocking(move || {
            let mut tree = me.load_tree(&doc.doc_id)?;
            tree.entry = Some(doc.clone());
            me.cache
                .write()
                .insert(doc.doc_id.clone(), clone_tree(&tree));
            me.save_tree(&doc.doc_id, &tree)?;
            me.save_index()?;
            Ok(())
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn bm25_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        substring_search(self, query, limit, None).await
    }

    async fn bm25_search_in_doc(
        &self,
        doc_id: &DocId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        substring_search(self, query, limit, Some(doc_id)).await
    }

    async fn put_raw(&self, doc_id: &DocId, data: &[u8]) -> Result<u64> {
        let path = self.raw_path(doc_id);
        let me = self.clone();
        let doc_id = doc_id.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            let offset = file.metadata()?.len();
            file.write_all(&data)?;
            file.flush()?;
            drop(me);
            drop(doc_id);
            Ok(offset)
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn read_raw_span(&self, doc_id: &DocId, span: (u64, u64)) -> Result<Vec<u8>> {
        if span.0 > span.1 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "span {span:?} start > end"
            )));
        }
        let path = self.raw_path(doc_id);
        let doc_id = doc_id.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Seek, SeekFrom};
            let mut file = std::fs::File::open(&path)
                .map_err(|_| PagebridgeError::DocumentNotFound(doc_id.clone()))?;
            file.seek(SeekFrom::Start(span.0))?;
            let mut buf = vec![0u8; (span.1 - span.0) as usize];
            file.read_exact(&mut buf)?;
            Ok(buf)
        })
        .await
        .map_err(|e| err("join", e))?
    }

    async fn get_summary_cache(&self, hash: &[u8; 32]) -> Result<Option<SummaryCacheEntry>> {
        Ok(self.summaries.read().get(hash).cloned())
    }

    async fn upsert_summary_cache(&self, hash: &[u8; 32], entry: &SummaryCacheEntry) -> Result<()> {
        self.summaries.write().insert(*hash, entry.clone());
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.save_summaries())
            .await
            .map_err(|e| err("join", e))?
    }

    async fn stats(&self) -> Result<AdapterStats> {
        let cache = self.cache.read();
        let node_count = cache.values().map(|t| t.nodes.len() as u64).sum();
        let document_count = cache.values().filter(|t| t.entry.is_some()).count() as u64;
        let mut raw_bytes = 0u64;
        if let Ok(read_dir) = std::fs::read_dir(self.root.join("raw")) {
            for entry in read_dir.flatten() {
                if let Ok(md) = entry.metadata() {
                    raw_bytes += md.len();
                }
            }
        }
        let summary_cache_entries = self.summaries.read().len() as u64;
        Ok(AdapterStats {
            node_count,
            document_count,
            raw_bytes,
            summary_cache_entries,
        })
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }
}

async fn substring_search(
    me: &JsonFileAdapter,
    query: &str,
    limit: usize,
    filter: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    let q_terms = tokenize(query);
    let cache = me.cache.read();
    let mut hits = Vec::new();
    let trees: Vec<&DocTree> = match filter {
        Some(d) => cache.get(d).into_iter().collect(),
        None => cache.values().collect(),
    };
    for tree in trees {
        for n in tree.nodes.values() {
            if !n.is_leaf {
                continue;
            }
            let haystack = format!(
                "{} {} {} {}",
                n.title,
                n.routing_summary,
                n.summary,
                n.keywords.join(" ")
            );
            let score = score_overlap(&q_terms, &tokenize(&haystack));
            if score > 0.0 {
                hits.push(SearchHit {
                    node_id: n.node_id.clone(),
                    doc_id: n.doc_id.clone(),
                    title: n.title.clone(),
                    score,
                });
            }
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    Ok(hits)
}
