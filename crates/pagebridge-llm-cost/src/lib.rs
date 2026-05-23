//! Per-provider per-model token cost catalog.
//!
//! Each entry records input/output prices per 1M tokens in micro-USD
//! (integer math, no floats in storage). The bundled catalog is the
//! community-maintained snapshot at the time of release; production
//! deployments can refresh it at runtime.

#![allow(clippy::missing_errors_doc)]

use std::collections::BTreeMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PriceEntry {
    pub provider: String,
    pub model: String,
    pub input_per_1m_micro_usd: u64,
    pub output_per_1m_micro_usd: u64,
    pub effective_date: String,
}

pub struct CostCatalog {
    inner: RwLock<BTreeMap<(String, String), PriceEntry>>,
}

impl CostCatalog {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            inner: RwLock::new(BTreeMap::new()),
        }
    }

    #[must_use]
    pub fn bundled() -> Self {
        let c = Self::empty();
        let json = include_str!("bundled.json");
        if let Ok(entries) = serde_json::from_str::<Vec<PriceEntry>>(json) {
            for e in entries {
                c.insert(e);
            }
        }
        c
    }

    pub fn insert(&self, entry: PriceEntry) {
        self.inner
            .write()
            .insert((entry.provider.clone(), entry.model.clone()), entry);
    }

    #[must_use]
    pub fn lookup(&self, provider: &str, model: &str) -> Option<PriceEntry> {
        self.inner
            .read()
            .get(&(provider.to_string(), model.to_string()))
            .cloned()
    }

    /// Compute the USD micro-cost for a given token count split.
    #[must_use]
    pub fn cost_micro_usd(
        &self,
        provider: &str,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
    ) -> Option<u64> {
        let e = self.lookup(provider, model)?;
        let i = u64::from(input_tokens) * e.input_per_1m_micro_usd / 1_000_000;
        let o = u64::from(output_tokens) * e.output_per_1m_micro_usd / 1_000_000;
        Some(i + o)
    }
}

impl Default for CostCatalog {
    fn default() -> Self {
        Self::bundled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_cost_work() {
        let c = CostCatalog::empty();
        c.insert(PriceEntry {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            input_per_1m_micro_usd: 150_000,
            output_per_1m_micro_usd: 600_000,
            effective_date: "2026-01-01".into(),
        });
        let cost = c.cost_micro_usd("openai", "gpt-4o-mini", 1_000_000, 1_000_000).unwrap();
        assert_eq!(cost, 150_000 + 600_000);
    }

    #[test]
    fn bundled_loads_without_panic() {
        let _ = CostCatalog::bundled();
    }
}
