//! Integration tests for the MySQL/MariaDB adapter, using testcontainers.
//!
//! Requires Docker. The test runner pulls a small MariaDB image automatically.

#![allow(clippy::redundant_clone)]

use pagebridge_adapter_mysql::MySqlAdapter;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::{DocumentEntry, SummaryCacheEntry};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mariadb::Mariadb;

async fn start_mariadb() -> Option<(testcontainers::ContainerAsync<Mariadb>, String)> {
    let container = match Mariadb::default().start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping mariadb testcontainer (docker unavailable): {e}");
            return None;
        }
    };
    let host = container.get_host().await.ok()?;
    let port = container.get_host_port_ipv4(3306).await.ok()?;
    let url = format!("mysql://root@{host}:{port}/test");
    Some((container, url))
}

fn make_root(doc: &DocId) -> NodeRecord {
    NodeRecord {
        node_id: NodeId::root(doc),
        doc_id: doc.clone(),
        parent_id: None,
        title: format!("Document {doc}"),
        level: NodeLevel::Document,
        routing_summary: "doc root".into(),
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
    }
}

fn make_section(doc: &DocId, sec: u32, title: &str) -> NodeRecord {
    let root = NodeId::root(doc);
    let sec_id = root.child("sec", &sec.to_string()).unwrap();
    NodeRecord {
        node_id: sec_id,
        doc_id: doc.clone(),
        parent_id: Some(root),
        title: title.into(),
        level: NodeLevel::Section,
        routing_summary: format!("toc for {title}"),
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
    }
}

fn make_leaf(doc: &DocId, sec: u32, leaf: u32, title: &str, kw: &[&str]) -> NodeRecord {
    let root = NodeId::root(doc);
    let sec_id = root.child("sec", &sec.to_string()).unwrap();
    let leaf_id = sec_id.child("leaf", &leaf.to_string()).unwrap();
    NodeRecord {
        node_id: leaf_id,
        doc_id: doc.clone(),
        parent_id: Some(sec_id),
        title: title.into(),
        level: NodeLevel::Leaf,
        routing_summary: format!("toc for {title}"),
        summary: format!(
            "body of {title} discussing implementation timeline rollout and budget specifics"
        ),
        child_ids: vec![],
        span: Some((0, 32)),
        page_start: Some(1),
        page_end: Some(1),
        keywords: kw.iter().map(|s| (*s).to_owned()).collect(),
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    }
}

#[tokio::test]
async fn full_mysql_roundtrip() {
    let Some((_container, url)) = start_mariadb().await else {
        return;
    };
    let adapter = MySqlAdapter::connect(&url).await.unwrap();
    adapter.migrate().await.unwrap();
    adapter.ping().await.unwrap();

    let doc = DocId::new("d1").unwrap();
    adapter.upsert_node(&make_root(&doc)).await.unwrap();
    adapter
        .upsert_node(&make_section(&doc, 1, "Intro"))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 1, "Timeline", &["timeline", "rollout"]))
        .await
        .unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 2, "Budget", &["budget", "cost"]))
        .await
        .unwrap();
    adapter
        .upsert_document(&DocumentEntry {
            doc_id: doc.clone(),
            title: "Doc 1".into(),
            source_kind: "markdown".into(),
            ingested_at: 1,
            root_node_id: NodeId::root(&doc),
            leaf_count: 2,
            byte_count: 0,
            raw_text_hash: None,
            structural_hash: None,
            document_type: None,
        })
        .await
        .unwrap();

    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);

    let leaves = adapter.leaves_under(&NodeId::root(&doc)).await.unwrap();
    assert_eq!(leaves.len(), 2);

    let hits = adapter.bm25_search("timeline rollout", 10).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.score >= 0.0));

    // Raw chunked roundtrip across a chunk boundary.
    let payload = b"abcdefghij".repeat(50);
    let off = adapter.put_raw(&doc, &payload).await.unwrap();
    assert_eq!(off, 0);
    let read = adapter
        .read_raw_text(&doc, (0, payload.len() as u64))
        .await
        .unwrap();
    assert_eq!(read.len(), payload.len());

    let h = [3u8; 32];
    adapter
        .upsert_summary_cache(
            &h,
            &SummaryCacheEntry {
                routing_summary: "rs".into(),
                summary: "s".into(),
                keywords: vec!["k".into()],
                model_fingerprint: "m".into(),
                created_at: 1,
            },
        )
        .await
        .unwrap();
    assert!(adapter.get_summary_cache(&h).await.unwrap().is_some());

    adapter.delete_document(&doc).await.unwrap();
    assert!(adapter.list_documents().await.unwrap().is_empty());
}
