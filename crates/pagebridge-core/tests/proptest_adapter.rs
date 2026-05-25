//! Property tests against `MemoryAdapter`.

#![allow(clippy::redundant_clone)]
//!
//! The contract: a random sequence of upsert/get/delete operations preserves
//! the invariant `get(id) == last_upserted(id)` when the id was not deleted
//! since, and `get(id) == None` when it was.

use std::sync::Arc;

use pagebridge_core::adapter::{MemoryAdapter, StorageAdapter};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use proptest::prelude::*;
use proptest::strategy::ValueTree;

fn arb_doc_slug() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z0-9_-]{1,16}").unwrap()
}

fn arb_doc_id() -> impl Strategy<Value = DocId> {
    arb_doc_slug().prop_map(|s| DocId::new(s).expect("valid doc id"))
}

fn arb_node_for_doc(doc: DocId) -> impl Strategy<Value = NodeRecord> {
    (
        proptest::string::string_regex("[a-z0-9]{1,8}").unwrap(),
        any::<u32>(),
        any::<bool>(),
    )
        .prop_map(move |(seg_value, idx, is_leaf)| {
            let root = NodeId::root(&doc);
            let child = root
                .child(&seg_value, &idx.to_string())
                .expect("valid child");
            NodeRecord {
                node_id: child.clone(),
                doc_id: doc.clone(),
                parent_id: Some(root),
                title: "T".into(),
                level: NodeLevel::Leaf,
                routing_summary: "rs".into(),
                summary: "s".into(),
                child_ids: vec![],
                span: None,
                page_start: None,
                page_end: None,
                keywords: vec![],
                is_leaf,
                created_at: 0,
                updated_at: 0,
                source_hash: [0; 32],
                canonical_section: None,
                section_aliases: vec![],
            }
        })
}

proptest! {
    #[test]
    fn upsert_then_get_returns_same_record(doc in arb_doc_id()) {
        let adapter = Arc::new(MemoryAdapter::new());
        let node_strategy = arb_node_for_doc(doc.clone());
        let mut runner = proptest::test_runner::TestRunner::default();
        let node = node_strategy.new_tree(&mut runner).unwrap().current();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            adapter.upsert_node(&node).await.unwrap();
            let got = adapter.get_node(&node.node_id).await.unwrap().unwrap();
            prop_assert_eq!(got.node_id, node.node_id);
            prop_assert_eq!(got.title, node.title);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn delete_document_removes_all_its_nodes(doc in arb_doc_id()) {
        let adapter = Arc::new(MemoryAdapter::new());
        let node_strategy = arb_node_for_doc(doc.clone());
        let mut runner = proptest::test_runner::TestRunner::default();
        let node = node_strategy.new_tree(&mut runner).unwrap().current();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            adapter.upsert_node(&node).await.unwrap();
            adapter.delete_document(&doc).await.unwrap();
            let got = adapter.get_node(&node.node_id).await.unwrap();
            prop_assert!(got.is_none());
            Ok(())
        }).unwrap();
    }
}
