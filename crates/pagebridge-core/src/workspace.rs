//! Workspace identifiers for multi-tenant deployments.
//!
//! A workspace partitions documents and queries within a single pagebridge
//! instance. In v0.3.0 the public API surface is workspace-aware via
//! [`crate::WorkspaceHandle`]; underlying storage adapters share a single
//! tablespace. True per-workspace storage isolation lands in v0.4.0 with
//! the per-adapter migration to add a `workspace_id` column.

use std::fmt;

use crate::error::{PagebridgeError, Result};

/// Identifier for a workspace. Same charset rules as [`crate::DocId`]:
/// lowercase ASCII alphanumeric plus `-` and `_`, length 1..=64.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Construct a workspace id, validating the slug.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// The implicit "default" workspace used when callers do not specify one.
    #[must_use]
    pub fn default_workspace() -> Self {
        Self("default".to_owned())
    }

    /// Borrow the underlying slug.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(s: &str) -> Result<()> {
        if s.is_empty() || s.len() > 64 {
            return Err(PagebridgeError::InvalidArgument(format!(
                "invalid workspace id: {s:?}"
            )));
        }
        for ch in s.chars() {
            let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_';
            if !ok {
                return Err(PagebridgeError::InvalidArgument(format!(
                    "invalid workspace id: {s:?}"
                )));
            }
        }
        Ok(())
    }
}

impl Default for WorkspaceId {
    fn default() -> Self {
        Self::default_workspace()
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workspace_is_default() {
        assert_eq!(WorkspaceId::default_workspace().as_str(), "default");
    }

    #[test]
    fn rejects_uppercase_and_punctuation() {
        assert!(WorkspaceId::new("ACME").is_err());
        assert!(WorkspaceId::new("space with spaces").is_err());
        assert!(WorkspaceId::new("").is_err());
    }

    #[test]
    fn accepts_typical_slugs() {
        for s in ["acme", "team-1", "project_42", "a"] {
            assert!(WorkspaceId::new(s).is_ok(), "rejected {s}");
        }
    }
}
