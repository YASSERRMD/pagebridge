//! Snapshot cadence policy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPolicy {
    /// Take a snapshot every N audit events. Default: 10_000.
    pub cadence_events: u32,
    /// Also take a snapshot every N seconds, even if cadence_events
    /// is not yet reached. Default: 86400 (one day).
    pub cadence_seconds: u32,
    /// Retain at most N snapshots; older ones are evicted.
    pub retain: u32,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            cadence_events: 10_000,
            cadence_seconds: 86_400,
            retain: 30,
        }
    }
}
