//! Identifier types: [`DocId`] and [`NodeId`].
//!
//! `DocId` is a short, user-facing slug for a document (lowercase alnum plus `-` `_`,
//! 1 to 64 chars). `NodeId` is a hierarchical path of `kind:value` segments rooted at
//! a document: `doc:<doc_id>/<segment>/<segment>...`. NodeIds are lexicographically
//! ordered, which makes the children of a parent contiguous under any backing store
//! that range-scans by byte order (used by [`crate`] adapters).

use std::fmt;
use std::str::FromStr;

use crate::error::{PagebridgeError, Result};

/// Slug identifying a document. Lowercase alphanumeric plus `-` and `_`, length 1..=64.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct DocId(String);

impl DocId {
    /// Create a `DocId`, validating the input. Returns `InvalidDocId` on failure.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(s: &str) -> Result<()> {
        if s.is_empty() || s.len() > 64 {
            return Err(PagebridgeError::InvalidDocId(s.to_owned()));
        }
        for ch in s.chars() {
            let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_';
            if !ok {
                return Err(PagebridgeError::InvalidDocId(s.to_owned()));
            }
        }
        Ok(())
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for DocId {
    type Err = PagebridgeError;
    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

/// Hierarchical, immutable identifier for a node in the tree.
///
/// Format: `doc:<doc_id>/<segment>/<segment>...` where each segment is `kind:value`.
/// Examples:
/// - `doc:carbon-policy-2026` (the document root)
/// - `doc:carbon-policy-2026/sec:1.2/leaf:7` (a leaf within section 1.2)
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct NodeId(String);

impl NodeId {
    /// Construct a node id from a raw string, validating the structure.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Construct the root node id for a document.
    #[must_use]
    pub fn root(doc: &DocId) -> Self {
        Self(format!("doc:{}", doc.as_str()))
    }

    /// Build a child node id by appending a `kind:value` segment.
    pub fn child(&self, kind: &str, value: &str) -> Result<Self> {
        validate_segment_part(kind, "kind")?;
        validate_segment_part(value, "value")?;
        Ok(Self(format!("{}/{}:{}", self.0, kind, value)))
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parent node id, or `None` if this is the document root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let idx = self.0.rfind('/')?;
        Some(Self(self.0[..idx].to_owned()))
    }

    /// Depth in the tree. The document root has depth 0; each `/` segment adds 1.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.bytes().filter(|b| *b == b'/').count()
    }

    /// Extract the underlying [`DocId`].
    pub fn doc_id(&self) -> Result<DocId> {
        let first = self
            .0
            .split('/')
            .next()
            .ok_or_else(|| PagebridgeError::InvalidNodeId(self.0.clone()))?;
        let slug = first
            .strip_prefix("doc:")
            .ok_or_else(|| PagebridgeError::InvalidNodeId(self.0.clone()))?;
        DocId::new(slug)
    }

    /// Returns true if this id is a prefix of `other` (i.e. `other` is in our subtree).
    #[must_use]
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        other.0 == self.0 || other.0.starts_with(&format!("{}/", self.0))
    }

    fn validate(s: &str) -> Result<()> {
        if s.is_empty() || s.len() > 512 {
            return Err(PagebridgeError::InvalidNodeId(s.to_owned()));
        }
        let mut segments = s.split('/');
        let head = segments
            .next()
            .ok_or_else(|| PagebridgeError::InvalidNodeId(s.to_owned()))?;
        let doc_slug = head
            .strip_prefix("doc:")
            .ok_or_else(|| PagebridgeError::InvalidNodeId(s.to_owned()))?;
        // Reuse DocId rules.
        DocId::new(doc_slug).map_err(|_| PagebridgeError::InvalidNodeId(s.to_owned()))?;
        for seg in segments {
            let (kind, value) = seg
                .split_once(':')
                .ok_or_else(|| PagebridgeError::InvalidNodeId(s.to_owned()))?;
            validate_segment_part(kind, "kind")
                .map_err(|_| PagebridgeError::InvalidNodeId(s.to_owned()))?;
            validate_segment_part(value, "value")
                .map_err(|_| PagebridgeError::InvalidNodeId(s.to_owned()))?;
        }
        Ok(())
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for NodeId {
    type Err = PagebridgeError;
    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

fn validate_segment_part(s: &str, what: &str) -> Result<()> {
    if s.is_empty() || s.len() > 64 {
        return Err(PagebridgeError::InvalidArgument(format!(
            "segment {what} must be 1..=64 chars: {s:?}"
        )));
    }
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        if !ok {
            return Err(PagebridgeError::InvalidArgument(format!(
                "segment {what} has invalid char {ch:?}: {s:?}"
            )));
        }
    }
    Ok(())
}
