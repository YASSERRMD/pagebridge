//! Auto-classification at ingest. Detectors scan content and return a
//! [`SensitivityLabel`] if anything triggers.

use regex::Regex;

use crate::label::SensitivityLabel;

pub trait LabelDetector: Send + Sync + 'static {
    fn detect(&self, content: &str) -> Option<SensitivityLabel>;
}

pub struct RegexDetector {
    email: Regex,
    ssn: Regex,
    phone: Regex,
    credit_card: Regex,
}

impl RegexDetector {
    #[must_use]
    pub fn new() -> Self {
        Self {
            email: Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap(),
            ssn: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
            phone: Regex::new(r"\b\+?\d{1,3}[\s\-]?\(?\d{3}\)?[\s\-]?\d{3,4}[\s\-]?\d{4}\b").unwrap(),
            credit_card: Regex::new(r"\b(?:\d[ -]*?){13,19}\b").unwrap(),
        }
    }
}

impl Default for RegexDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelDetector for RegexDetector {
    fn detect(&self, content: &str) -> Option<SensitivityLabel> {
        let mut cats = Vec::new();
        if self.email.is_match(content) {
            cats.push("email".to_string());
        }
        if self.ssn.is_match(content) {
            cats.push("ssn".to_string());
        }
        if self.phone.is_match(content) {
            cats.push("phone".to_string());
        }
        if self.credit_card.is_match(content) {
            cats.push("credit_card".to_string());
        }
        if cats.is_empty() {
            None
        } else {
            Some(SensitivityLabel::Pii { categories: cats })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_email_and_ssn() {
        let d = RegexDetector::new();
        let label = d.detect("contact: jane@example.com, ssn 123-45-6789").unwrap();
        match label {
            SensitivityLabel::Pii { categories } => {
                assert!(categories.contains(&"email".to_string()));
                assert!(categories.contains(&"ssn".to_string()));
            }
            other => panic!("expected PII, got {other:?}"),
        }
    }

    #[test]
    fn plain_text_returns_none() {
        let d = RegexDetector::new();
        assert!(d.detect("hello world").is_none());
    }
}
