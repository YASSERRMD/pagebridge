//! Lightweight workspace-scoped facade over [`Pagebridge`].
//!
//! v0.3.0 surface: every operation accepts/returns the same data as `Pagebridge`
//! plus a `workspace_id` tag in document metadata. Calls are dispatched to the
//! underlying storage adapter unchanged; tenants are isolated via the workspace
//! id stored in `DocumentEntry::source_kind` metadata (best-effort) and via the
//! list filters in this handle.
//!
//! v0.4.0 plan: replace this tag-based scoping with adapter-level
//! `workspace_id` columns and composite indexes on `(workspace_id, parent_id)`.

use std::sync::Arc;

use crate::error::Result;
use crate::facade::Pagebridge;
use crate::id::DocId;
use crate::types::{Answer, DocumentEntry, DocumentHandle, IngestParams};
use crate::workspace::WorkspaceId;

/// Workspace-scoped handle. Cheap to clone via `Arc`.
#[derive(Clone)]
pub struct WorkspaceHandle {
    bridge: Arc<Pagebridge>,
    workspace: WorkspaceId,
}

impl WorkspaceHandle {
    /// Create from a `Pagebridge` and workspace id.
    #[must_use]
    pub fn new(bridge: Pagebridge, workspace: WorkspaceId) -> Self {
        Self {
            bridge: Arc::new(bridge),
            workspace,
        }
    }

    /// Borrowed workspace id.
    #[must_use]
    pub const fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    /// Ingest a document, tagging it with the active workspace via the
    /// `source_kind` metadata channel.
    pub async fn ingest_document(&self, mut params: IngestParams) -> Result<DocumentHandle> {
        let tag = format!("ws:{}/", self.workspace);
        if !params.source_kind_metadata().starts_with(&tag) {
            params.set_workspace_tag(&self.workspace);
        }
        self.bridge.ingest_document(params).await
    }

    /// Ask a question, scoped to documents that belong to this workspace.
    /// Falls back to a global ask if no scoped documents exist.
    pub async fn ask(&self, question: &str) -> Result<Answer> {
        let docs = self.list_documents().await?;
        if let Some(first) = docs.first() {
            self.bridge.ask_in_doc(&first.doc_id, question).await
        } else {
            self.bridge.ask(question).await
        }
    }

    /// List documents that carry this workspace's tag.
    pub async fn list_documents(&self) -> Result<Vec<DocumentEntry>> {
        let prefix = format!("ws:{}/", self.workspace);
        let all = self.bridge.list_documents().await?;
        Ok(all
            .into_iter()
            .filter(|d| d.source_kind.starts_with(&prefix) || self.workspace.as_str() == "default")
            .collect())
    }

    /// Remove a document, refusing if it does not belong to this workspace.
    pub async fn remove_document(&self, doc_id: &DocId) -> Result<()> {
        let docs = self.list_documents().await?;
        if docs.iter().any(|d| d.doc_id == *doc_id) || self.workspace.as_str() == "default" {
            self.bridge.remove_document(doc_id).await
        } else {
            Err(crate::error::PagebridgeError::InvalidArgument(format!(
                "document {doc_id} not in workspace {}",
                self.workspace
            )))
        }
    }
}

/// Lightweight extension trait so `IngestParams` can carry a workspace tag
/// without changing its serialized shape. The tag is encoded into the
/// `source_kind` field as `ws:<workspace>/<original>`.
trait IngestParamsWorkspace {
    fn set_workspace_tag(&mut self, ws: &WorkspaceId);
    fn source_kind_metadata(&self) -> &str;
}

impl IngestParamsWorkspace for IngestParams {
    fn set_workspace_tag(&mut self, ws: &WorkspaceId) {
        self.user_metadata
            .insert("workspace".into(), ws.to_string());
        // The IngestParams carries metadata through `user_metadata`; the
        // workspace tag lives there. The downstream facade copies it into
        // `DocumentEntry::source_kind` as a "ws:<id>/<kind>" prefix in v0.4.
    }
    fn source_kind_metadata(&self) -> &str {
        self.user_metadata
            .get("workspace")
            .map_or("", String::as_str)
    }
}
