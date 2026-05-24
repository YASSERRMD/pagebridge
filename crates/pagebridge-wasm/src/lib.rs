//! Browser-side pagebridge via WebAssembly.
//!
//! When compiled to `wasm32-unknown-unknown` with the `browser`
//! feature, this crate exposes the pagebridge API to JavaScript via
//! `wasm-bindgen`. Storage uses OPFS (Origin Private File System);
//! the LLM bridge calls out to Transformers.js or any WebGPU runtime
//! the host page provides.
//!
//! Native builds expose a tiny shim API so the integration test suite
//! can exercise the boundary without a browser.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_pass_by_value
)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    pub question: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResponseShim {
    pub answer: String,
    pub citations: Vec<String>,
    pub used_node_ids: Vec<String>,
}

/// Native test shim: returns a deterministic synthetic answer so the
/// browser bridge contract can be exercised without an LLM.
#[must_use]
pub fn ask_shim(req: AskRequest) -> AskResponseShim {
    AskResponseShim {
        answer: format!("(shim) you asked: {}", req.question),
        citations: vec![],
        used_node_ids: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_round_trips() {
        let r = ask_shim(AskRequest {
            question: "what is the limit?".into(),
            workspace_id: "default".into(),
        });
        assert!(r.answer.contains("what is the limit?"));
    }
}
