//! Versioned prompt library. In-flight queries lock the prompt version
//! at start; subsequent swaps do not retroactively affect them.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Hot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedPrompts {
    pub version: u32,
    pub by_name: BTreeMap<String, String>,
}

impl VersionedPrompts {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            version: 1,
            by_name: BTreeMap::new(),
        }
    }
}

/// Snapshot a prompt library at the start of a query so the query
/// cannot observe a mid-flight swap.
#[must_use]
pub fn snapshot(hot: &Hot<VersionedPrompts>) -> Arc<VersionedPrompts> {
    hot.load()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_query_keeps_old_prompts() {
        let h = Hot::new(VersionedPrompts {
            version: 1,
            by_name: [("synth".into(), "old".into())].into_iter().collect(),
        });
        let snap = snapshot(&h);
        h.swap(VersionedPrompts {
            version: 2,
            by_name: [("synth".into(), "new".into())].into_iter().collect(),
        });
        assert_eq!(snap.by_name["synth"], "old");
        assert_eq!(h.load().by_name["synth"], "new");
    }
}
