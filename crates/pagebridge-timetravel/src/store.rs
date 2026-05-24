//! Persistent snapshot store. Snapshots are stored as JSON files named
//! `<created_at_ns>-<snapshot_id_first_8>.json` so they sort
//! chronologically.

use std::path::PathBuf;

use async_trait::async_trait;

use pagebridge_deterministic::CorpusSnapshot;

use crate::error::Result;
#[cfg(test)]
use crate::error::TimeTravelError;

#[async_trait]
pub trait SnapshotStore: Send + Sync + 'static {
    async fn put(&self, snapshot: &CorpusSnapshot) -> Result<()>;
    async fn list_before(&self, ts_ns: u128) -> Result<Vec<CorpusSnapshot>>;
    async fn latest(&self) -> Result<Option<CorpusSnapshot>>;
}

pub struct FileSnapshotStore {
    base: PathBuf,
}

impl FileSnapshotStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    async fn ensure_dir(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.base).await?;
        Ok(())
    }

    fn path_for(&self, snapshot: &CorpusSnapshot) -> PathBuf {
        let short = snapshot
            .snapshot_id
            .get(0..8)
            .unwrap_or(&snapshot.snapshot_id)
            .to_string();
        self.base
            .join(format!("{:020}-{}.json", snapshot.created_at_ns, short))
    }
}

#[async_trait]
impl SnapshotStore for FileSnapshotStore {
    async fn put(&self, snapshot: &CorpusSnapshot) -> Result<()> {
        self.ensure_dir().await?;
        let path = self.path_for(snapshot);
        let bytes = serde_json::to_vec_pretty(snapshot)?;
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    async fn list_before(&self, ts_ns: u128) -> Result<Vec<CorpusSnapshot>> {
        self.ensure_dir().await?;
        let mut out = Vec::new();
        let mut read = tokio::fs::read_dir(&self.base).await?;
        while let Some(entry) = read.next_entry().await? {
            let bytes = tokio::fs::read(entry.path()).await?;
            let s: CorpusSnapshot = serde_json::from_slice(&bytes)?;
            if s.created_at_ns <= ts_ns {
                out.push(s);
            }
        }
        out.sort_by_key(|s| s.created_at_ns);
        Ok(out)
    }

    async fn latest(&self) -> Result<Option<CorpusSnapshot>> {
        let v = self.list_before(u128::MAX).await?;
        Ok(v.into_iter().next_back())
    }
}

/// In-memory store for tests.
pub struct MemorySnapshotStore {
    inner: parking_lot::Mutex<Vec<CorpusSnapshot>>,
}

impl MemorySnapshotStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemorySnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SnapshotStore for MemorySnapshotStore {
    async fn put(&self, snapshot: &CorpusSnapshot) -> Result<()> {
        self.inner.lock().push(snapshot.clone());
        Ok(())
    }
    async fn list_before(&self, ts_ns: u128) -> Result<Vec<CorpusSnapshot>> {
        let g = self.inner.lock();
        let mut v: Vec<_> = g
            .iter()
            .filter(|s| s.created_at_ns <= ts_ns)
            .cloned()
            .collect();
        v.sort_by_key(|s| s.created_at_ns);
        Ok(v)
    }
    async fn latest(&self) -> Result<Option<CorpusSnapshot>> {
        let v = self.list_before(u128::MAX).await?;
        Ok(v.into_iter().next_back())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagebridge_core::workspace::WorkspaceId;

    fn snap(ts: u128) -> CorpusSnapshot {
        let ws = WorkspaceId::new("acme").unwrap();
        let mut s = CorpusSnapshot::new(ws, vec![]);
        s.created_at_ns = ts;
        s
    }

    #[tokio::test]
    async fn memory_store_returns_only_snapshots_at_or_before_ts() {
        let store = MemorySnapshotStore::new();
        store.put(&snap(1)).await.unwrap();
        store.put(&snap(2)).await.unwrap();
        store.put(&snap(3)).await.unwrap();
        let before = store.list_before(2).await.unwrap();
        assert_eq!(before.len(), 2);
        let latest = store.latest().await.unwrap().unwrap();
        assert_eq!(latest.created_at_ns, 3);
        let _ = TimeTravelError::Internal("ok".into());
    }
}
