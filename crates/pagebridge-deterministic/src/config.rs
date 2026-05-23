//! Configuration for deterministic mode.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How adapters must order query results to be deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryOrder {
    /// Order by primary key ascending.
    ByPrimaryKey,
    /// Order by content hash ascending (useful for BM25 tie-breaks).
    ByContentHash,
    /// Order by node_id lexicographically.
    ByNodeId,
}

impl Default for QueryOrder {
    fn default() -> Self {
        Self::ByNodeId
    }
}

/// Master switch + per-layer pins. Pass this into pagebridge to switch
/// the appliance to deterministic mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicMode {
    pub enabled: bool,
    pub llm_seed: u64,
    pub llm_temperature_milli: u32,
    pub llm_top_p_milli: u32,
    pub adapter_query_order: QueryOrder,
    pub prompt_version_pin: BTreeMap<String, u32>,
    pub navigation_policy_pin: u32,
    /// If set, every query must match this snapshot id; if it doesn't, the
    /// facade returns `DeterministicError::SnapshotMismatch`.
    pub require_snapshot: Option<String>,
}

impl Default for DeterministicMode {
    fn default() -> Self {
        Self {
            enabled: false,
            llm_seed: 0,
            llm_temperature_milli: 0,
            llm_top_p_milli: 1000,
            adapter_query_order: QueryOrder::ByNodeId,
            prompt_version_pin: BTreeMap::new(),
            navigation_policy_pin: 1,
            require_snapshot: None,
        }
    }
}

impl DeterministicMode {
    #[must_use]
    pub fn strict() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off() {
        let d = DeterministicMode::default();
        assert!(!d.enabled);
    }

    #[test]
    fn strict_is_on_with_default_pins() {
        let d = DeterministicMode::strict();
        assert!(d.enabled);
        assert_eq!(d.llm_temperature_milli, 0);
    }
}
