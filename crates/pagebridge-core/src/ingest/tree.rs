//! Shared tree utilities. v0.1 keeps these light because the per-source parsers
//! produce their own NodeRecord vectors directly.

use crate::record::NodeRecord;

/// Walk records in depth-first order based on their NodeId ordering.
#[must_use]
pub fn depth_first(records: &[NodeRecord]) -> Vec<&NodeRecord> {
    let mut out: Vec<&NodeRecord> = records.iter().collect();
    out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    out
}
