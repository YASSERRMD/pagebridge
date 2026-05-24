//! Reconstruct a historical snapshot by picking the nearest
//! prior stored snapshot and applying the audit log forward up to
//! the requested timestamp.

use async_trait::async_trait;

use pagebridge_core::workspace::WorkspaceId;
use pagebridge_deterministic::CorpusSnapshot;

use crate::error::{Result, TimeTravelError};
use crate::overlay::{MutationEvent, Overlay};
use crate::store::SnapshotStore;

/// Source of mutation events between two timestamps. Implementations
/// translate the audit log (or whatever else they have) into a stream
/// of [`MutationEvent`]s.
#[async_trait]
pub trait MutationSource: Send + Sync + 'static {
    async fn between(
        &self,
        workspace: &WorkspaceId,
        from_ns: u128,
        to_ns: u128,
    ) -> Result<Vec<MutationEvent>>;
}

/// Recover the snapshot live at `ts_ns`. Picks the nearest snapshot at
/// or before `ts_ns`, then forward-replays mutations from
/// `snapshot.created_at_ns` to `ts_ns` over an overlay.
pub async fn snapshot_at<S: SnapshotStore, M: MutationSource>(
    store: &S,
    mutations: &M,
    workspace: WorkspaceId,
    ts_ns: u128,
) -> Result<CorpusSnapshot> {
    let mut candidates = store.list_before(ts_ns).await?;
    let base = candidates.pop().ok_or(TimeTravelError::NoSnapshotBefore)?;
    let events = mutations
        .between(&workspace, base.created_at_ns, ts_ns)
        .await?;
    let mut overlay = Overlay::from_snapshot(&base);
    overlay.apply_forward(events);
    Ok(overlay.as_snapshot(workspace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemorySnapshotStore;
    use pagebridge_core::id::{DocId, NodeId};
    use pagebridge_deterministic::SnapshotEntry;
    use std::sync::Arc;

    fn entry(name: &str, hash: &str, v: u32) -> SnapshotEntry {
        let doc = DocId::new("doc").unwrap();
        SnapshotEntry {
            node_id: NodeId::root(&doc).child("sec", name).unwrap(),
            content_hash_hex: hash.into(),
            version: v,
        }
    }

    struct ScriptedMutations(Vec<(u128, MutationEvent)>);
    #[async_trait]
    impl MutationSource for ScriptedMutations {
        async fn between(
            &self,
            _workspace: &WorkspaceId,
            from_ns: u128,
            to_ns: u128,
        ) -> Result<Vec<MutationEvent>> {
            Ok(self
                .0
                .iter()
                .filter(|(ts, _)| *ts > from_ns && *ts <= to_ns)
                .map(|(_, e)| e.clone())
                .collect())
        }
    }

    #[tokio::test]
    async fn snapshot_at_recovers_state_at_timestamp() {
        let ws = WorkspaceId::new("acme").unwrap();
        let store = MemorySnapshotStore::new();
        let mut base = CorpusSnapshot::new(ws.clone(), vec![entry("a", "11", 1)]);
        base.created_at_ns = 100;
        store.put(&base).await.unwrap();

        // Between t=100 and t=200, insert b. Between t=200 and t=300,
        // update a. We ask for t=250 -> should see b inserted but not the
        // update.
        let muts = ScriptedMutations(vec![
            (150, MutationEvent::Insert(entry("b", "22", 1))),
            (250, MutationEvent::Update(entry("a", "33", 2))),
        ]);

        let snap = snapshot_at(&store, &muts, ws, 200).await.unwrap();
        let a = snap
            .entries
            .iter()
            .find(|e| e.node_id.as_str().ends_with(":a"))
            .unwrap();
        assert_eq!(a.content_hash_hex, "11");
        assert!(snap
            .entries
            .iter()
            .any(|e| e.node_id.as_str().ends_with(":b")));
        let _ = Arc::<u32>::default;
    }
}
