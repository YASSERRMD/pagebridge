//! Phase J6: query intent classification and BM25 query expansion.
//!
//! When the document type is known the raw question string is analysed to
//! detect which canonical section the user is asking about.  The matched
//! section's canonical name and a subset of its aliases are then appended
//! to the query so BM25 ranks leaves from the right section higher.
//!
//! ## Example
//!
//! Query: "What languages do you know?"
//! Document type: `Resume`
//! Matched section: `skills` (aliases: "technical skills", "core competencies", ...)
//! Expanded query: "What languages do you know? skills technical skills core competencies"

use crate::section_schema::{document_schema_for, DocumentSchema, SectionSchema};
use crate::types::DocumentType;

/// Configuration for query intent classification and expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryIntentConfig {
    /// Enable intent classification and query expansion. Default: `true`.
    pub enabled: bool,
    /// Maximum number of aliases to append during expansion. Default: `3`.
    pub max_aliases: usize,
}

impl Default for QueryIntentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_aliases: 3,
        }
    }
}

/// Classify a query against a document schema to find the most likely target section.
///
/// Checks every schema section in declaration order and returns the first one
/// whose canonical name or any alias appears as a whole token in `query`
/// (case-insensitive). Returns `None` if no section matches.
///
/// The `doc_type` parameter is used by callers to select the correct schema
/// via [`document_schema_for`]; it is not used directly here so that this
/// function can also be called with an externally-provided schema.
pub fn classify_query_intent(
    query: &str,
    _doc_type: DocumentType,
    schema: &'static DocumentSchema,
) -> Option<&'static SectionSchema> {
    let lower = query.to_lowercase();
    schema.sections.iter().find(|sec| {
        if contains_token(&lower, sec.canonical_name) {
            return true;
        }
        sec.aliases
            .iter()
            .any(|alias| contains_token(&lower, alias))
    })
}

/// Expand a query by appending a section's canonical name and key aliases.
///
/// Only terms that are not already present in the query are appended.
/// At most `max_aliases` alias terms are added (in addition to the
/// canonical name).
pub fn expand_query(query: &str, section: &'static SectionSchema, max_aliases: usize) -> String {
    let lower = query.to_lowercase();
    let mut extra: Vec<&str> = Vec::new();

    if !contains_token(&lower, section.canonical_name) {
        extra.push(section.canonical_name);
    }
    for alias in section.aliases {
        if extra.len() > max_aliases {
            break;
        }
        if !contains_token(&lower, alias) {
            extra.push(alias);
        }
    }

    if extra.is_empty() {
        query.to_owned()
    } else {
        format!("{} {}", query, extra.join(" "))
    }
}

/// Classify and optionally expand a query for a given document type.
///
/// Returns the original query unchanged when:
/// - `cfg.enabled` is `false`,
/// - `doc_type` is `Generic` (no type-specific schema applies), or
/// - no section in the schema matches the query.
pub fn maybe_expand(query: &str, doc_type: DocumentType, cfg: QueryIntentConfig) -> String {
    if !cfg.enabled || doc_type == DocumentType::Generic {
        return query.to_owned();
    }
    let schema = document_schema_for(doc_type);
    match classify_query_intent(query, doc_type, schema) {
        Some(sec) => expand_query(query, sec, cfg.max_aliases),
        None => query.to_owned(),
    }
}

/// Returns `true` if `haystack` (already lowercased) contains `needle` as a
/// whole-token substring bounded by non-alphanumeric characters or string ends.
///
/// Multi-word needles are matched as contiguous substrings (e.g. "technical skills").
fn contains_token(haystack: &str, needle: &str) -> bool {
    let needle_lower = needle.to_lowercase();
    let nlen = needle_lower.len();
    if nlen == 0 {
        return false;
    }
    let hb = haystack.as_bytes();
    let nb = needle_lower.as_bytes();
    let hlen = hb.len();

    let mut i = 0usize;
    while i + nlen <= hlen {
        if hb[i..i + nlen] == *nb {
            let left_ok = i == 0 || !hb[i - 1].is_ascii_alphanumeric();
            let right_ok = i + nlen == hlen || !hb[i + nlen].is_ascii_alphanumeric();
            if left_ok && right_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DocumentType;

    fn resume_schema() -> &'static DocumentSchema {
        document_schema_for(DocumentType::Resume)
    }

    // -------------------------------------------------------------------
    // classify_query_intent
    // -------------------------------------------------------------------

    #[test]
    fn classify_skills_canonical() {
        let sec = classify_query_intent(
            "What are your technical skills?",
            DocumentType::Resume,
            resume_schema(),
        );
        assert_eq!(sec.map(|s| s.canonical_name), Some("skills"));
    }

    #[test]
    fn classify_experience_via_alias() {
        let sec = classify_query_intent(
            "List your work experience",
            DocumentType::Resume,
            resume_schema(),
        );
        assert_eq!(sec.map(|s| s.canonical_name), Some("experience"));
    }

    #[test]
    fn classify_no_match_returns_none() {
        let sec = classify_query_intent(
            "How old is the universe?",
            DocumentType::Resume,
            resume_schema(),
        );
        assert!(sec.is_none());
    }

    #[test]
    fn classify_education_canonical() {
        // "Tell me about your education" would match "summary" first (alias "about").
        // Use a query that hits "education" cleanly.
        let sec = classify_query_intent(
            "What degrees do you hold?",
            DocumentType::Resume,
            resume_schema(),
        );
        assert_eq!(sec.map(|s| s.canonical_name), Some("education"));
    }

    // -------------------------------------------------------------------
    // expand_query
    // -------------------------------------------------------------------

    #[test]
    fn expand_appends_canonical_and_aliases() {
        let schema = resume_schema();
        let skills_sec = schema
            .sections
            .iter()
            .find(|s| s.canonical_name == "skills")
            .unwrap();
        let expanded = expand_query("What languages do you know?", skills_sec, 2);
        assert!(
            expanded.starts_with("What languages do you know?"),
            "original query must be preserved: {expanded}"
        );
        assert!(
            expanded.contains("skills"),
            "canonical name must appear: {expanded}"
        );
    }

    #[test]
    fn expand_does_not_duplicate_present_term() {
        let schema = resume_schema();
        let skills_sec = schema
            .sections
            .iter()
            .find(|s| s.canonical_name == "skills")
            .unwrap();
        let expanded = expand_query("skills test", skills_sec, 3);
        // The original query must be at the beginning.
        assert!(
            expanded.starts_with("skills test"),
            "original must be preserved: {expanded}"
        );
        // The canonical name must NOT be the first appended token right after the
        // original (it was already present so expand_query should skip it and go
        // straight to aliases like "technical skills").
        let appended = expanded.trim_start_matches("skills test").trim();
        assert!(
            !appended.starts_with("skills ") && appended != "skills",
            "canonical 'skills' must not be the first appended token: {expanded}"
        );
    }

    // -------------------------------------------------------------------
    // maybe_expand
    // -------------------------------------------------------------------

    #[test]
    fn maybe_expand_disabled_returns_original() {
        let cfg = QueryIntentConfig {
            enabled: false,
            max_aliases: 3,
        };
        let q = "What are your skills?";
        assert_eq!(maybe_expand(q, DocumentType::Resume, cfg), q);
    }

    #[test]
    fn maybe_expand_generic_type_returns_original() {
        let cfg = QueryIntentConfig::default();
        let q = "Tell me about yourself";
        assert_eq!(maybe_expand(q, DocumentType::Generic, cfg), q);
    }

    #[test]
    fn maybe_expand_no_match_returns_original() {
        let cfg = QueryIntentConfig::default();
        let q = "How old is the universe?";
        assert_eq!(maybe_expand(q, DocumentType::Resume, cfg), q);
    }

    #[test]
    fn maybe_expand_matched_expands_query() {
        let cfg = QueryIntentConfig::default();
        let q = "What are your work experience highlights?";
        let expanded = maybe_expand(q, DocumentType::Resume, cfg);
        assert!(
            expanded.len() > q.len(),
            "expanded query should be longer: {expanded}"
        );
    }

    // -------------------------------------------------------------------
    // contains_token
    // -------------------------------------------------------------------

    #[test]
    fn token_match_at_boundaries() {
        assert!(contains_token("i know rust", "rust"));
        assert!(!contains_token("trusted", "rust"));
        assert!(!contains_token("rustacean", "rust"));
    }

    #[test]
    fn token_match_multi_word() {
        assert!(contains_token(
            "what are your technical skills here",
            "technical skills"
        ));
        assert!(!contains_token("technical skillset", "technical skills"));
    }

    #[test]
    fn token_match_at_start_and_end() {
        assert!(contains_token("skills assessment", "skills"));
        assert!(contains_token("assess skills", "skills"));
    }
}
