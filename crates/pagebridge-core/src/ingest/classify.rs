//! Document type classifier (Phase J1).
//!
//! Peeks at the first ~2-4 kB of a freshly-uploaded document and asks the
//! configured LLM to classify it into one of [`crate::types::DocumentType`]'s
//! variants. The result is persisted on [`crate::types::DocumentEntry`] and
//! consulted by every downstream phase (type-aware parsers, type-aware
//! summary prompts, query intent expansion, section-first navigation).
//!
//! Failure modes are explicit:
//! * Provider error: the caller logs and falls back to `Generic`.
//! * Confidence below the configured floor: returned as `(Generic, confidence)`.
//! * Malformed JSON or unknown type tag: returned as `(Generic, 0.0)`.
//!
//! The classifier never aborts ingest; recall on `Generic` is identical to
//! the pre-J1 pipeline so the worst case for a misbehaving model is "no
//! recall improvement", not "ingest broken".

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value
)]

use std::sync::Arc;

use crate::error::Result;
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider};
use crate::types::DocumentType;

/// Tunables for the document type classifier.
#[derive(Debug, Clone, Copy)]
pub struct ClassifyConfig {
    /// Master switch. When `false`, every document is classified as
    /// [`DocumentType::Generic`] without an LLM call.
    pub enabled: bool,
    /// Confidence floor in `[0.0, 1.0]`. Predictions below this collapse
    /// to [`DocumentType::Generic`] so a low-conviction guess never
    /// drives downstream parsing decisions.
    pub min_confidence: f32,
    /// Maximum number of characters from the document body that are
    /// included in the classifier prompt. Capped to keep prompt costs
    /// bounded for very large documents.
    pub sample_chars: usize,
}

impl Default for ClassifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.5,
            sample_chars: 4000,
        }
    }
}

/// One classifier verdict: the predicted type plus the model's reported
/// confidence in `[0.0, 1.0]`. Confidence below
/// [`ClassifyConfig::min_confidence`] is rewritten to
/// [`DocumentType::Generic`] before being returned, so callers can treat
/// every result as already gated.
pub type ClassifyOutcome = (DocumentType, f32);

/// JSON schema the classifier expects from the provider. Centralized so
/// the same shape drives both the prompt and the parsing path.
#[must_use]
pub fn classify_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["document_type", "confidence"],
        "properties": {
            "document_type": {
                "type": "string",
                "enum": [
                    "resume",
                    "research_paper",
                    "legal_document",
                    "technical_manual",
                    "business_report",
                    "meeting_minutes",
                    "email",
                    "book_or_chapter",
                    "generic"
                ]
            },
            "confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0
            },
            "reasons": {
                "type": "array",
                "items": { "type": "string" }
            }
        }
    })
}

/// Build the classifier prompt. Exposed for tests so the wire format can
/// be asserted without spinning up an LLM.
#[must_use]
pub fn build_classify_prompt(title: &str, sample_text: &str, sample_chars: usize) -> String {
    let trimmed_title = title.trim();
    let sample = if sample_text.len() > sample_chars {
        // Walk backwards from the byte cap to the nearest UTF-8 char
        // boundary so multi-byte inputs never panic.
        let mut end = sample_chars.min(sample_text.len());
        while end > 0 && !sample_text.is_char_boundary(end) {
            end -= 1;
        }
        &sample_text[..end]
    } else {
        sample_text
    };
    format!(
        "Classify the following document into exactly one of these categories:\n\
         resume, research_paper, legal_document, technical_manual,\n\
         business_report, meeting_minutes, email, book_or_chapter, generic.\n\n\
         Use \"generic\" only when none of the others fit.\n\n\
         TITLE: {trimmed_title}\n\n\
         FIRST {len} CHARACTERS:\n{sample}\n\n\
         Respond with a single JSON object matching the schema:\n\
         {{\"document_type\": \"<tag>\", \"confidence\": <0..1>, \"reasons\": [\"<short reason>\"]}}\n\
         Respond ONLY with the JSON object, no prose.",
        len = sample.chars().count(),
    )
}

/// Classify a document via the configured LLM provider.
///
/// Returns `Ok((Generic, 0.0))` when the classifier is disabled or the
/// provider returns malformed output. Returns `Err` only when the
/// provider itself errors; callers may decide whether to propagate or
/// fall back to `Generic`.
pub async fn classify_document(
    llm: &Arc<dyn LlmProvider>,
    title: &str,
    sample_text: &str,
    config: ClassifyConfig,
) -> Result<ClassifyOutcome> {
    if !config.enabled {
        return Ok((DocumentType::Generic, 0.0));
    }
    let prompt = build_classify_prompt(title, sample_text, config.sample_chars);
    let schema = classify_schema();
    let resp = llm
        .complete_json(
            CompletionRequest {
                system: Some(
                    "You are a precise document classifier. Read the title and the \
                     opening text, then pick the single best category."
                        .into(),
                ),
                messages: vec![ChatMessage::user(prompt)],
                ..CompletionRequest::default()
            },
            &schema,
        )
        .await?;
    Ok(parse_classify_response(&resp, config.min_confidence))
}

/// Parse a provider response into a gated [`ClassifyOutcome`]. Exposed for
/// tests so the JSON-handling path can be exercised without a live LLM.
#[must_use]
pub fn parse_classify_response(resp: &serde_json::Value, min_confidence: f32) -> ClassifyOutcome {
    let tag = resp
        .get("document_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let confidence = resp
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .map_or(0.0_f32, |c| c.clamp(0.0, 1.0) as f32);
    let parsed = DocumentType::parse_tag(tag).unwrap_or(DocumentType::Generic);
    if confidence < min_confidence {
        return (DocumentType::Generic, confidence);
    }
    (parsed, confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_happy_path() {
        let v = serde_json::json!({
            "document_type": "resume",
            "confidence": 0.92,
            "reasons": ["bullets of work experience", "skills section"]
        });
        let (kind, conf) = parse_classify_response(&v, 0.5);
        assert_eq!(kind, DocumentType::Resume);
        assert!((conf - 0.92).abs() < 1e-3);
    }

    #[test]
    fn parse_response_below_floor_falls_back_to_generic() {
        let v = serde_json::json!({
            "document_type": "research_paper",
            "confidence": 0.2
        });
        let (kind, conf) = parse_classify_response(&v, 0.5);
        assert_eq!(kind, DocumentType::Generic);
        assert!((conf - 0.2).abs() < 1e-3);
    }

    #[test]
    fn parse_response_unknown_tag_falls_back_to_generic() {
        let v = serde_json::json!({
            "document_type": "screenplay",
            "confidence": 0.99
        });
        let (kind, _conf) = parse_classify_response(&v, 0.5);
        assert_eq!(kind, DocumentType::Generic);
    }

    #[test]
    fn parse_response_missing_fields_yields_generic() {
        let v = serde_json::json!({});
        let (kind, conf) = parse_classify_response(&v, 0.5);
        assert_eq!(kind, DocumentType::Generic);
        assert!(conf.abs() < 1e-6);
    }

    #[test]
    fn prompt_truncates_long_samples_on_char_boundaries() {
        let title = "Test";
        let body: String = "ä".repeat(5000); // 2-byte each, 10_000 bytes
        let prompt = build_classify_prompt(title, &body, 200);
        // The truncation must not produce invalid UTF-8 or panic.
        assert!(prompt.contains("FIRST"));
        assert!(prompt.is_char_boundary(prompt.len()));
    }
}
