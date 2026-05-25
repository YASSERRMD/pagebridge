//! Integration tests for the SQL Server adapter, using a generic testcontainers image.
//!
//! This test is gated by the `MSSQL_TEST=1` environment variable since the
//! Microsoft container image is large and licensing requires acknowledgment.
//! To run locally: `MSSQL_TEST=1 cargo test -p pagebridge-adapter-mssql --test mssql`.

#![allow(clippy::redundant_clone)]

use pagebridge_adapter_mssql::MSSqlAdapter;
use pagebridge_core::adapter::StorageAdapter;
use pagebridge_core::id::{DocId, NodeId};
use pagebridge_core::record::{NodeLevel, NodeRecord};
use pagebridge_core::types::{DocumentEntry, SummaryCacheEntry};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

const PASSWORD: &str = "yourStrong(!)Password";

async fn start_mssql() -> (ContainerAsync<GenericImage>, String) {
    let image = GenericImage::new("mcr.microsoft.com/mssql/server", "2022-latest")
        .with_exposed_port(1433.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "SQL Server is now ready for client connections",
        ));
    let container = image
        .with_env_var("ACCEPT_EULA", "Y")
        .with_env_var("MSSQL_PID", "Developer")
        .with_env_var("MSSQL_SA_PASSWORD", PASSWORD)
        .start()
        .await
        .unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(1433).await.unwrap();
    let conn = format!(
        "server=tcp:{host},{port};user=sa;password={PASSWORD};database=master;TrustServerCertificate=true"
    );
    (container, conn)
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

fn make_leaf(doc: &DocId, sec: u32, leaf: u32, title: &str) -> NodeRecord {
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
        summary: format!("body of {title} describing rollout timeline"),
        child_ids: vec![],
        span: Some((0, 16)),
        page_start: Some(1),
        page_end: Some(1),
        keywords: vec!["timeline".into()],
        is_leaf: true,
        created_at: 0,
        updated_at: 0,
        source_hash: [0; 32],
    }
}

#[tokio::test]
async fn full_mssql_roundtrip() {
    if std::env::var("MSSQL_TEST").ok().as_deref() != Some("1") {
        eprintln!("skipping MSSQL test: set MSSQL_TEST=1 to enable");
        return;
    }
    let (_container, conn) = start_mssql().await;
    let adapter = MSSqlAdapter::from_ado_string(&conn).await.unwrap();
    adapter.migrate().await.unwrap();
    adapter.ping().await.unwrap();

    let doc = DocId::new("d1").unwrap();
    adapter.upsert_node(&make_root(&doc)).await.unwrap();
    adapter
        .upsert_node(&make_leaf(&doc, 1, 1, "Timeline"))
        .await
        .unwrap();
    adapter
        .upsert_document(&DocumentEntry {
            doc_id: doc.clone(),
            title: "Doc 1".into(),
            source_kind: "markdown".into(),
            ingested_at: 1,
            root_node_id: NodeId::root(&doc),
            leaf_count: 1,
            byte_count: 0,
            raw_text_hash: None,
            structural_hash: None,
            document_type: None,
        })
        .await
        .unwrap();

    let docs = adapter.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);

    let hits = adapter.bm25_search("timeline", 5).await.unwrap();
    assert!(!hits.is_empty());

    let payload = b"hello world ".repeat(20);
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
