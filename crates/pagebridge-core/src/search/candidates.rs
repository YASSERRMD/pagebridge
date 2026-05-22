//! BM25 candidate selection: run the adapter's native FTS, return top hits.

use std::sync::Arc;

use crate::adapter::StorageAdapter;
use crate::error::Result;
use crate::id::DocId;
use crate::types::SearchHit;

/// Run BM25 search either globally or scoped to a single document.
pub async fn select(
    storage: &Arc<dyn StorageAdapter>,
    query: &str,
    limit: usize,
    doc: Option<&DocId>,
) -> Result<Vec<SearchHit>> {
    match doc {
        Some(d) => storage.bm25_search_in_doc(d, query, limit).await,
        None => storage.bm25_search(query, limit).await,
    }
}
