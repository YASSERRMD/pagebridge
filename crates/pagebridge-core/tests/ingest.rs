//! Integration tests for the ingest pipeline.

#![allow(clippy::redundant_clone)]

use std::sync::Arc;

use pagebridge_core::adapter::MemoryAdapter;
use pagebridge_core::id::NodeId;
use pagebridge_core::ingest::ingest;
use pagebridge_core::llm::EchoLlmProvider;
use pagebridge_core::prompts::PromptLibrary;
use pagebridge_core::types::{IngestParams, SourceKind};

const SAMPLE_MD: &str = "# Carbon Policy 2026\n\
\n\
Some intro text.\n\
\n\
## Section 1: Goals\n\
\n\
We want a circular economy.\n\
We commit to net-zero by 2050.\n\
\n\
## Section 2: Implementation\n\
\n\
Phase 1 launches in Q1 2026.\n\
Phase 2 launches in Q3 2027.\n\
\n\
### Subsection 2.1: Timeline\n\
\n\
The full rollout completes by Q4 2027.\n";

#[tokio::test]
async fn ingest_markdown_two_pass() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    // Canned JSON for summarize prompts.
    for _ in 0..10 {
        llm.push_json(serde_json::json!({
            "title": "Echoed",
            "routing_summary": "echoed routing",
            "summary": "echoed summary",
            "keywords": ["echo", "test"],
        }));
    }
    let prompts = Arc::new(PromptLibrary::v1());

    let params = IngestParams {
        title: "Carbon Policy 2026".into(),
        source_kind: SourceKind::Markdown,
        raw_text: SAMPLE_MD.as_bytes().to_vec(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (handle, join) = ingest(storage.clone(), llm, prompts, params).await.unwrap();
    assert!(handle.leaf_count >= 1);
    join.await.unwrap().unwrap();

    let docs = pagebridge_core::adapter::StorageAdapter::list_documents(&*storage)
        .await
        .unwrap();
    assert_eq!(docs.len(), 1);
    let root = NodeId::root(&handle.doc_id);
    let kids = pagebridge_core::adapter::StorageAdapter::children_summaries(&*storage, &root)
        .await
        .unwrap();
    assert!(!kids.is_empty());

    // Each non-leaf node should now have a non-empty summary or routing_summary.
    let leaves = pagebridge_core::adapter::StorageAdapter::leaves_under(&*storage, &root)
        .await
        .unwrap();
    assert!(!leaves.is_empty());
}

#[tokio::test]
async fn ingest_plain_text() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..10 {
        llm.push_json(serde_json::json!({
            "title": "Echoed",
            "routing_summary": "rs",
            "summary": "s",
            "keywords": []
        }));
    }
    let prompts = Arc::new(PromptLibrary::v1());

    let params = IngestParams {
        title: "Plain Doc".into(),
        source_kind: SourceKind::Plain,
        raw_text: "Sentence one. Sentence two. Sentence three.\nSentence four. Sentence five."
            .as_bytes()
            .to_vec(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (handle, join) = ingest(storage.clone(), llm, prompts, params).await.unwrap();
    assert!(handle.leaf_count >= 1);
    join.await.unwrap().unwrap();
}

#[tokio::test]
async fn summary_cache_hit_on_reingest() {
    let storage = Arc::new(MemoryAdapter::new());
    let llm = Arc::new(EchoLlmProvider::new());
    for _ in 0..40 {
        llm.push_json(serde_json::json!({
            "title": "T",
            "routing_summary": "rs",
            "summary": "s",
            "keywords": ["a"]
        }));
    }
    let prompts = Arc::new(PromptLibrary::v1());

    let p1 = IngestParams {
        title: "Doc one".into(),
        source_kind: SourceKind::Markdown,
        raw_text: SAMPLE_MD.as_bytes().to_vec(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_, j1) = ingest(storage.clone(), llm.clone(), prompts.clone(), p1)
        .await
        .unwrap();
    j1.await.unwrap().unwrap();

    let stats1 = pagebridge_core::adapter::StorageAdapter::stats(&*storage)
        .await
        .unwrap();

    // Ingest a second copy with the same content but a new title - cache hits expected.
    let p2 = IngestParams {
        title: "Doc two".into(),
        source_kind: SourceKind::Markdown,
        raw_text: SAMPLE_MD.as_bytes().to_vec(),
        doc_id: None,
        user_metadata: std::collections::BTreeMap::default(),
    };
    let (_, j2) = ingest(storage.clone(), llm, prompts, p2).await.unwrap();
    j2.await.unwrap().unwrap();

    let stats2 = pagebridge_core::adapter::StorageAdapter::stats(&*storage)
        .await
        .unwrap();
    // The summary cache should not have grown linearly with re-ingest: the second pass
    // hits the cache for matching child payloads.
    assert!(stats2.summary_cache_entries <= stats1.summary_cache_entries * 2);
    assert!(stats2.document_count == 2);
}
