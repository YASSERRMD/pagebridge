//! Unit tests for the JSON-schema to GBNF lowering.

use pagebridge_llm_llamacpp::grammar::schema_to_gbnf;
use serde_json::json;

#[test]
fn nested_object_grammar_is_well_formed() {
    let schema = json!({
        "type": "object",
        "required": ["action", "leaves"],
        "properties": {
            "action": {"enum": ["descend", "halt"]},
            "leaves": {"type": "array", "items": {"type": "integer"}},
            "note": {"type": "string"}
        }
    });
    let g = schema_to_gbnf(&schema);
    // Required fields appear unwrapped.
    assert!(g.contains("\"\\\"action\\\"\""));
    assert!(g.contains("\"\\\"leaves\\\"\""));
    // Optional field is wrapped in (...)? form.
    assert!(g.contains("\"\\\"note\\\"\""));
    // Primitive rules are present.
    assert!(g.contains("string ::="));
    assert!(g.contains("integer ::="));
    assert!(g.contains("json_value ::="));
}

#[test]
fn stub_constructor_reports_disabled() {
    use pagebridge_core::llm::{CompletionRequest, LlmProvider};
    use pagebridge_llm_llamacpp::LlamaCppProvider;
    // Stub-mode test only: when the driver feature is on we can't synthesize
    // a real GGUF here, so the test focuses on the disabled-driver contract.
    #[cfg(not(feature = "llamacpp-driver"))]
    {
        let dir = tempfile::tempdir().unwrap_or_else(|_| panic!("tempdir"));
        let path = dir.path().join("model.gguf");
        std::fs::write(&path, b"not really a gguf").unwrap();
        let provider = LlamaCppProvider::from_gguf(&path).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(provider.complete(CompletionRequest::user("hi")));
        assert!(res.is_err());
        let msg = format!("{}", res.unwrap_err());
        assert!(msg.to_lowercase().contains("driver not enabled"));
    }
    #[cfg(feature = "llamacpp-driver")]
    {
        // When the real driver is on, just confirm the public surface compiles.
        let _ = LlamaCppProvider::from_gguf;
    }
}
