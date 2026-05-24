//! Historical-query correctness: ingest at t1, mutate at t2, ingest at t3,
//! ask state at every checkpoint, confirm the overlay matches the
//! expected state at each point.

use async_trait::async_trait;

use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::workspace::WorkspaceId;
use pagebridge_deterministic::{CorpusSnapshot, SnapshotEntry};
use pagebridge_timetravel::{
    snapshot_at, MemorySnapshotStore, MutationEvent, MutationSource, Result, SnapshotStore,
};

fn entry(name: &str, hash: &str, v: u32) -> SnapshotEntry {
    let doc = DocId::new("doc").unwrap();
    SnapshotEntry {
        node_id: NodeId::root(&doc).child("sec", name).unwrap(),
        content_hash_hex: hash.into(),
        version: v,
    }
}

struct Scripted(Vec<(u128, MutationEvent)>);

#[async_trait]
impl MutationSource for Scripted {
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
async fn t1_t2_t3_history_walk() {
    let ws = WorkspaceId::new("acme").unwrap();
    let store = MemorySnapshotStore::new();

    // Initial state at t=100: { a:11 }
    let mut base = CorpusSnapshot::new(ws.clone(), vec![entry("a", "11", 1)]);
    base.created_at_ns = 100;
    store.put(&base).await.unwrap();

    let muts = Scripted(vec![
        (150, MutationEvent::Insert(entry("b", "22", 1))), // t=150 add b
        (200, MutationEvent::Update(entry("a", "33", 2))), // t=200 update a
        (
            250,
            MutationEvent::Delete {
                // t=250 delete b
                node_id: entry("b", "22", 1).node_id,
            },
        ),
    ]);

    // At t=120: only a:11 (no muts yet after base).
    let s120 = snapshot_at(&store, &muts, ws.clone(), 120).await.unwrap();
    assert_eq!(s120.entries.len(), 1);
    assert_eq!(s120.entries[0].content_hash_hex, "11");

    // At t=175: a:11 + b:22
    let s175 = snapshot_at(&store, &muts, ws.clone(), 175).await.unwrap();
    assert_eq!(s175.entries.len(), 2);

    // At t=225: a:33 + b:22
    let s225 = snapshot_at(&store, &muts, ws.clone(), 225).await.unwrap();
    let a = s225
        .entries
        .iter()
        .find(|e| e.node_id.as_str().ends_with(":a"))
        .unwrap();
    assert_eq!(a.content_hash_hex, "33");
    assert_eq!(s225.entries.len(), 2);

    // At t=300: only a:33 (b deleted).
    let s300 = snapshot_at(&store, &muts, ws, 300).await.unwrap();
    assert_eq!(s300.entries.len(), 1);
    assert_eq!(s300.entries[0].content_hash_hex, "33");
}
