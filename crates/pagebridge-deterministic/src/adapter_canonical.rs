//! Canonical SQL fragments that adapters bolt onto every query to
//! produce a deterministic order. Each adapter calls `order_by_for` to
//! get the trailing ORDER BY clause appropriate for its dialect and the
//! requested `QueryOrder`.

use crate::config::QueryOrder;

#[must_use]
pub fn order_by_for(order: QueryOrder) -> &'static str {
    match order {
        QueryOrder::ByPrimaryKey => " ORDER BY node_id ASC",
        QueryOrder::ByContentHash => " ORDER BY content_hash ASC, node_id ASC",
        QueryOrder::ByNodeId => " ORDER BY node_id ASC",
    }
}

/// Canonical "secondary" order: tie-breaker used by every BM25 result
/// so identical-score hits land in a stable order across runs.
#[must_use]
pub fn tiebreaker_for(order: QueryOrder) -> &'static str {
    match order {
        QueryOrder::ByContentHash => " content_hash ASC, node_id ASC",
        _ => " node_id ASC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_orders_emit_node_id() {
        for o in [
            QueryOrder::ByPrimaryKey,
            QueryOrder::ByContentHash,
            QueryOrder::ByNodeId,
        ] {
            assert!(order_by_for(o).contains("node_id"));
        }
    }
}
