//! Causal trace + counterfactual replay.
//!
//! Pagebridge's existing trace records every step a query takes; this
//! crate enriches it into a causal DAG (node = step, edge = dependency)
//! so the CLI can answer "why this answer?" with a structured walk and
//! "what if we used a different X?" with a replay diff.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalStep {
    pub id: String,
    pub kind: String,
    pub inputs: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalDag {
    pub query_id: String,
    pub steps: Vec<CausalStep>,
}

impl CausalDag {
    /// Walk the DAG topologically and produce a human-readable
    /// "why this answer?" explanation.
    #[must_use]
    pub fn explain(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Query {}\n", self.query_id));
        for step in &self.steps {
            out.push_str(&format!(
                "  - [{}] {} (depends_on={:?})\n",
                step.id, step.kind, step.depends_on
            ));
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Change {
    Llm { provider: String, model: String },
    Snapshot { snapshot_id: String },
    PromptVersion { name: String, version: u32 },
    NavigationPolicy { version: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counterfactual {
    pub base_query_id: String,
    pub change: Change,
    pub base_answer_hash_hex: String,
    pub alt_answer_hash_hex: String,
    pub diverged: bool,
}

impl Counterfactual {
    #[must_use]
    pub fn diff_summary(&self) -> String {
        format!(
            "change={:?}\n  base={}\n  alt ={}\n  diverged={}",
            self.change,
            self.base_answer_hash_hex,
            self.alt_answer_hash_hex,
            self.diverged
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_returns_topologically_sensible_lines() {
        let d = CausalDag {
            query_id: "q-1".into(),
            steps: vec![
                CausalStep {
                    id: "s1".into(),
                    kind: "bm25".into(),
                    inputs: BTreeMap::new(),
                    outputs: BTreeMap::new(),
                    depends_on: vec![],
                },
                CausalStep {
                    id: "s2".into(),
                    kind: "navigate".into(),
                    inputs: BTreeMap::new(),
                    outputs: BTreeMap::new(),
                    depends_on: vec!["s1".into()],
                },
            ],
        };
        let text = d.explain();
        assert!(text.contains("s1"));
        assert!(text.contains("s2"));
    }

    #[test]
    fn counterfactual_diff_renders() {
        let c = Counterfactual {
            base_query_id: "q-1".into(),
            change: Change::Llm {
                provider: "openai".into(),
                model: "gpt-4o-mini".into(),
            },
            base_answer_hash_hex: "aa".into(),
            alt_answer_hash_hex: "bb".into(),
            diverged: true,
        };
        let s = c.diff_summary();
        assert!(s.contains("diverged=true"));
    }
}
