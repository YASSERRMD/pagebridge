//! In-memory link store and resolution pass.
//!
//! v0.5.0 ships this as an in-memory soft graph that wraps a `Pagebridge`
//! handle. v0.6.0 will add per-adapter `pagebridge_links` schema and migrate
//! callers to a persistent store; the trait surface here stays stable.

use std::collections::HashMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use pagebridge_core::id::{DocId, NodeId};

use crate::detector::{DetectedLink, LinkKind};

/// One stored link, edge of the soft graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub from_node: NodeId,
    pub kind: LinkKind,
    pub raw_text: String,
    pub confidence: f32,
    pub to_node: Option<NodeId>,
    pub to_doc: Option<DocId>,
}

impl Link {
    #[must_use]
    pub fn from_detected(from_node: NodeId, detected: DetectedLink) -> Self {
        Self {
            from_node,
            kind: detected.kind,
            raw_text: detected.raw_text,
            confidence: detected.confidence,
            to_node: None,
            to_doc: None,
        }
    }

    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        self.to_node.is_some() || self.to_doc.is_some()
    }
}

/// In-memory soft graph keyed by source node.
#[derive(Default)]
pub struct LinkStore {
    inner: RwLock<HashMap<NodeId, Vec<Link>>>,
}

impl LinkStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one link.
    pub fn insert(&self, link: Link) {
        self.inner
            .write()
            .entry(link.from_node.clone())
            .or_default()
            .push(link);
    }

    /// All outbound links from `node`.
    #[must_use]
    pub fn links_from(&self, node: &NodeId) -> Vec<Link> {
        self.inner.read().get(node).cloned().unwrap_or_default()
    }

    /// All inbound links to `node` (linear scan; the store is small in
    /// practice and the alternative cost is doubling the index size).
    #[must_use]
    pub fn links_to(&self, node: &NodeId) -> Vec<Link> {
        self.inner
            .read()
            .values()
            .flatten()
            .filter(|l| l.to_node.as_ref() == Some(node))
            .cloned()
            .collect()
    }

    /// Walk every unresolved link and attempt to satisfy it against the given
    /// document list. Returns the number of links resolved.
    pub fn resolve_against(&self, docs: &[(DocId, String, NodeId)]) -> u64 {
        let mut resolved = 0u64;
        {
            let mut guard = self.inner.write();
            for links in guard.values_mut() {
                for link in links.iter_mut() {
                    if link.is_resolved() {
                        continue;
                    }
                    if let Some((doc, _title, root)) = match_link(link, docs) {
                        link.to_doc = Some(doc.clone());
                        link.to_node = Some(root.clone());
                        resolved += 1;
                    }
                }
            }
        }
        resolved
    }

    /// Count of stored links.
    #[must_use]
    pub fn total(&self) -> usize {
        self.inner.read().values().map(Vec::len).sum()
    }
}

fn match_link<'a>(
    link: &Link,
    docs: &'a [(DocId, String, NodeId)],
) -> Option<&'a (DocId, String, NodeId)> {
    match link.kind {
        LinkKind::TitleRef => {
            let needle = link.raw_text.to_ascii_lowercase();
            docs.iter()
                .find(|(_, title, _)| title.to_ascii_lowercase().contains(&needle))
        }
        LinkKind::Url | LinkKind::Doi | LinkKind::Isbn | LinkKind::SectionRef => {
            // No automatic resolution for these yet; v0.6 will plumb in a URL
            // index and a section-id index. For now we mark them unresolved
            // and let the navigator surface them as is.
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::detect_all;

    fn make_link(node: &NodeId, raw: &str, kind: LinkKind) -> Link {
        Link {
            from_node: node.clone(),
            kind,
            raw_text: raw.to_owned(),
            confidence: 0.9,
            to_node: None,
            to_doc: None,
        }
    }

    #[test]
    fn insert_and_query_roundtrip() {
        let store = LinkStore::new();
        let doc = DocId::new("d1").unwrap();
        let node = NodeId::root(&doc);
        store.insert(make_link(&node, "Carbon policy", LinkKind::TitleRef));
        let out = store.links_from(&node);
        assert_eq!(out.len(), 1);
        assert_eq!(store.total(), 1);
    }

    #[test]
    fn resolve_title_ref_against_doc_list() {
        let store = LinkStore::new();
        let from_doc = DocId::new("from").unwrap();
        let to_doc = DocId::new("policy").unwrap();
        let from_node = NodeId::root(&from_doc);
        let to_root = NodeId::root(&to_doc);
        store.insert(make_link(&from_node, "Carbon policy", LinkKind::TitleRef));
        let docs = vec![(to_doc.clone(), "Carbon Policy 2026".into(), to_root.clone())];
        let count = store.resolve_against(&docs);
        assert_eq!(count, 1);
        let out = store.links_from(&from_node);
        assert_eq!(out[0].to_doc.as_ref(), Some(&to_doc));
    }

    #[test]
    fn detector_round_trip_into_store() {
        let store = LinkStore::new();
        let doc = DocId::new("d1").unwrap();
        let node = NodeId::root(&doc);
        for det in detect_all("see https://example.com and Section 3.2") {
            store.insert(Link::from_detected(node.clone(), det));
        }
        assert!(store.total() >= 2);
    }
}
