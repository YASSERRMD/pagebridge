//! Adapter write-throughput bench: compares per-node upsert vs batched
//! upsert_nodes_batch on the in-memory adapter. The other adapters share
//! the same trait shape; ratios should hold.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::format_push_string,
    clippy::redundant_clone,
    clippy::needless_pass_by_value,
    clippy::elidable_lifetime_names,
    clippy::manual_let_else,
    clippy::if_not_else,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_borrows_for_generic_args
)]

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pagebridge_core::adapter::{MemoryAdapter, StorageAdapter};
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use tokio::runtime::Runtime;

fn make_node(doc: &DocId, parent: &NodeId, seq: u32) -> NodeRecord {
    let node_id = parent.child("leaf", &seq.to_string()).unwrap();
    NodeRecord {
        node_id,
        doc_id: doc.clone(),
        parent_id: Some(parent.clone()),
        title: format!("Leaf {seq}"),
        level: NodeLevel::Leaf,
        routing_summary: "rs".into(),
        summary: "s".into(),
        child_ids: vec![],
        span: Some((0, 1)),
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
        canonical_section: None,
        section_aliases: vec![],
    }
}

fn root_record(doc: &DocId) -> NodeRecord {
    NodeRecord {
        node_id: NodeId::root(doc),
        doc_id: doc.clone(),
        parent_id: None,
        title: format!("Document {doc}"),
        level: NodeLevel::Document,
        routing_summary: "root".into(),
        summary: String::new(),
        child_ids: vec![],
        span: None,
        page_start: None,
        page_end: None,
        keywords: vec![],
        is_leaf: false,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
        canonical_section: None,
        section_aliases: vec![],
    }
}

fn bench_write_modes(c: &mut Criterion) {
    let rt = Runtime::new().expect("rt");
    let doc = DocId::new("doc-bench").unwrap();
    let root = NodeId::root(&doc);
    let mut group = c.benchmark_group("memory_adapter_writes");
    for count in [100u32, 1000, 5000] {
        let nodes: Vec<NodeRecord> = (0..count).map(|i| make_node(&doc, &root, i)).collect();
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::new("per_node", count), &nodes, |b, nodes| {
            b.iter(|| {
                rt.block_on(async {
                    let store = Arc::new(MemoryAdapter::new());
                    store.upsert_node(&root_record(&doc)).await.unwrap();
                    for n in nodes {
                        store.upsert_node(n).await.unwrap();
                    }
                });
            });
        });

        group.bench_with_input(BenchmarkId::new("batch", count), &nodes, |b, nodes| {
            b.iter(|| {
                rt.block_on(async {
                    let store = Arc::new(MemoryAdapter::new());
                    store.upsert_node(&root_record(&doc)).await.unwrap();
                    store.upsert_nodes_batch(nodes).await.unwrap();
                });
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_write_modes);
criterion_main!(benches);
