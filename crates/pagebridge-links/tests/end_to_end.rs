//! End-to-end tests for the soft-graph crate.

#![allow(clippy::redundant_clone)]

use pagebridge_core::id::{DocId, NodeId};
use pagebridge_links::{detect_all, Link, LinkKind, LinkStore};

#[test]
fn detect_and_resolve_title_reference_between_two_docs() {
    let store = LinkStore::new();
    let doc_a = DocId::new("doc-a").unwrap();
    let doc_b = DocId::new("doc-b").unwrap();
    let from_node = NodeId::root(&doc_a);
    let to_root = NodeId::root(&doc_b);

    // Manually emit a title-ref since the regex layer doesn't synthesize them
    // (the ingest pipeline that owns title resolution would attach them).
    store.insert(Link {
        from_node: from_node.clone(),
        kind: LinkKind::TitleRef,
        raw_text: "Carbon Policy".into(),
        confidence: 0.7,
        to_node: None,
        to_doc: None,
    });

    let docs = vec![(doc_b.clone(), "Carbon Policy 2026".into(), to_root.clone())];
    let resolved = store.resolve_against(&docs);
    assert_eq!(resolved, 1);

    let out = store.links_from(&from_node);
    assert!(out[0].is_resolved());
    let inbound = store.links_to(&to_root);
    assert_eq!(inbound.len(), 1);
}

#[test]
fn detector_finds_multiple_kinds() {
    let text = "see https://example.com/spec and DOI 10.1234/abcd-EFGH/9 plus Section 4.2.1";
    let links = detect_all(text);
    let kinds: Vec<LinkKind> = links.iter().map(|l| l.kind).collect();
    assert!(kinds.contains(&LinkKind::Url));
    assert!(kinds.contains(&LinkKind::Doi));
    assert!(kinds.contains(&LinkKind::SectionRef));
}
