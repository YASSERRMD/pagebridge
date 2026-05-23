//! Citation-pattern detection over node text.
//!
//! Today this is regex-driven: it catches URLs, DOIs, ISBNs, and inline
//! "Section X.Y" / "Sec. X.Y" references. Detection is deliberately liberal;
//! the resolution pass downstream is responsible for pruning false positives.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Classification of a detected reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    Url,
    Doi,
    Isbn,
    SectionRef,
    TitleRef,
}

/// One raw reference detected in a chunk of text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedLink {
    pub kind: LinkKind,
    pub raw_text: String,
    /// Confidence assigned by the detector, in `[0, 1]`. Resolution may
    /// upgrade or discard based on whether the link is satisfied.
    pub confidence: f32,
}

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+").expect("url regex")
});

static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)10\.\d{4,9}/[\-._;()/:A-Z0-9]+").expect("doi regex"));

static ISBN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:ISBN(?:-1[03])?:?\s*)?(?:\d{9}[\dX]|\d{10}|\d{13})",
    )
    .expect("isbn regex")
});

static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:see\s+)?(?:sec(?:tion|\.)?)\s+(\d+(?:\.\d+){0,3})")
        .expect("section regex")
});

/// Run every detector over `text` and return the merged list of references.
#[must_use]
pub fn detect_all(text: &str) -> Vec<DetectedLink> {
    let mut out = Vec::new();
    for m in URL_RE.find_iter(text) {
        out.push(DetectedLink {
            kind: LinkKind::Url,
            raw_text: m.as_str().to_owned(),
            confidence: 0.95,
        });
    }
    for m in DOI_RE.find_iter(text) {
        out.push(DetectedLink {
            kind: LinkKind::Doi,
            raw_text: m.as_str().to_owned(),
            confidence: 0.9,
        });
    }
    // ISBN requires checksum-ish length; trust the regex to a first approximation.
    for m in ISBN_RE.find_iter(text) {
        let raw = m.as_str().trim().to_owned();
        // Filter out the no-prefix 9-13-digit matches that overlap with phone
        // numbers or accession ids by demanding at least 10 digits.
        let digit_count = raw.chars().filter(char::is_ascii_digit).count();
        if (10..=13).contains(&digit_count) {
            out.push(DetectedLink {
                kind: LinkKind::Isbn,
                raw_text: raw,
                confidence: 0.6,
            });
        }
    }
    for m in SECTION_RE.captures_iter(text) {
        if let Some(num) = m.get(1) {
            out.push(DetectedLink {
                kind: LinkKind::SectionRef,
                raw_text: num.as_str().to_owned(),
                confidence: 0.5,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_url() {
        let links = detect_all("see https://example.com/path for details");
        assert!(links.iter().any(|l| l.kind == LinkKind::Url));
        assert!(links
            .iter()
            .any(|l| l.raw_text == "https://example.com/path"));
    }

    #[test]
    fn detects_doi() {
        let links = detect_all("cite 10.1234/abcd-EFGH/123 in your work");
        assert!(links.iter().any(|l| l.kind == LinkKind::Doi));
    }

    #[test]
    fn detects_section_reference() {
        let links = detect_all("see Section 3.2 of the policy");
        assert!(links
            .iter()
            .any(|l| matches!(l.kind, LinkKind::SectionRef) && l.raw_text == "3.2"));
    }

    #[test]
    fn empty_text_returns_no_links() {
        assert!(detect_all("").is_empty());
    }
}
