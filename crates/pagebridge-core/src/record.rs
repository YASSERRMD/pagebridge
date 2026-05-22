//! Node records: the heart of the cognitive tree.

use crate::error::{PagebridgeError, Result};
use crate::id::{DocId, NodeId};

/// Level of a node in the hierarchical tree.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeLevel {
    Corpus,
    Document,
    Section,
    Subsection,
    Page,
    Leaf,
}

/// Full record for a node in the tree. Stored verbatim by every adapter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeRecord {
    pub node_id: NodeId,
    pub doc_id: DocId,
    pub parent_id: Option<NodeId>,
    pub title: String,
    pub level: NodeLevel,
    /// 50-token TOC-style line used by the navigation prompt.
    pub routing_summary: String,
    /// 200-300 token chapter abstract used during synthesis.
    pub summary: String,
    pub child_ids: Vec<NodeId>,
    /// (start_byte, end_byte) into the raw document text. Set only for leaves.
    pub span: Option<(u64, u64)>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub keywords: Vec<String>,
    pub is_leaf: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_hash: [u8; 32],
}

impl NodeRecord {
    /// Validate structural invariants. Adapters should call this before persisting.
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(PagebridgeError::InvalidArgument(
                "NodeRecord.title must not be empty".into(),
            ));
        }
        if self.is_leaf && !self.child_ids.is_empty() {
            return Err(PagebridgeError::InvalidArgument(
                "leaf NodeRecord must have no child_ids".into(),
            ));
        }
        if !self.is_leaf && self.span.is_some() {
            return Err(PagebridgeError::InvalidArgument(
                "non-leaf NodeRecord must not have a span".into(),
            ));
        }
        if let Some((a, b)) = self.span {
            if a > b {
                return Err(PagebridgeError::InvalidArgument(format!(
                    "NodeRecord.span has start > end: ({a}, {b})"
                )));
            }
        }
        if let Some(parent) = &self.parent_id {
            if !parent.is_prefix_of(&self.node_id) {
                return Err(PagebridgeError::InvalidArgument(format!(
                    "NodeRecord.parent_id {} is not a prefix of node_id {}",
                    parent.as_str(),
                    self.node_id.as_str()
                )));
            }
        }
        let node_doc = self.node_id.doc_id()?;
        if node_doc != self.doc_id {
            return Err(PagebridgeError::InvalidArgument(format!(
                "NodeRecord.doc_id {} does not match node_id document {}",
                self.doc_id.as_str(),
                node_doc.as_str()
            )));
        }
        Ok(())
    }
}

/// Lightweight projection of [`NodeRecord`] used during navigation prompts.
/// Excludes the full summary and child list to keep the navigation prompt small.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeSummary {
    pub node_id: NodeId,
    pub parent_id: Option<NodeId>,
    pub title: String,
    pub level: NodeLevel,
    pub routing_summary: String,
    pub is_leaf: bool,
}

impl From<&NodeRecord> for NodeSummary {
    fn from(value: &NodeRecord) -> Self {
        Self {
            node_id: value.node_id.clone(),
            parent_id: value.parent_id.clone(),
            title: value.title.clone(),
            level: value.level,
            routing_summary: value.routing_summary.clone(),
            is_leaf: value.is_leaf,
        }
    }
}
