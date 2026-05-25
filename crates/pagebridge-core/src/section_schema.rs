//! Phase J2: type-specific section schemas.
//!
//! Each [`DocumentSchema`] describes the canonical sections that appear in a
//! given document type, the aliases that map real-world headings to those
//! canonical names, and the relative importance of each section for downstream
//! retrieval and summarization.
//!
//! ## Usage
//!
//! ```rust
//! use pagebridge_core::section_schema::{document_schema_for, SectionImportance};
//! use pagebridge_core::types::DocumentType;
//!
//! let schema = document_schema_for(DocumentType::Resume);
//! let sec = schema.canonical_for("work experience").unwrap();
//! assert_eq!(sec.canonical_name, "experience");
//! assert_eq!(sec.importance, SectionImportance::High);
//! ```

#![allow(
    clippy::missing_const_for_fn,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value
)]

use crate::record::NodeLevel;
use crate::types::DocumentType;

/// Relative importance of a section within its document type. Used by the
/// summarization and retrieval layers to weight section coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionImportance {
    /// Must be covered; directly answers most queries about this document type.
    High,
    /// Useful supporting context; included when query scope is broad.
    Medium,
    /// Peripheral; only surfaced for exhaustive queries.
    Low,
}

impl SectionImportance {
    /// Stable lowercase tag.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Describes one canonical section within a document type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionSchema {
    /// Normalized name used internally and in canonical section tagging.
    pub canonical_name: &'static str,
    /// Case-insensitive alternate headings that map to this section. Matching
    /// is done after lowercasing and stripping leading/trailing whitespace.
    pub aliases: &'static [&'static str],
    /// How important this section is for answering queries about this type.
    pub importance: SectionImportance,
    /// Expected tree level for nodes that belong to this section.
    pub expected_level: NodeLevel,
}

impl SectionSchema {
    /// Returns `true` when `heading` matches the canonical name or any alias
    /// (case-insensitive, after trimming whitespace).
    #[must_use]
    pub fn matches(&self, heading: &str) -> bool {
        let normalized = heading.trim().to_ascii_lowercase();
        if normalized == self.canonical_name {
            return true;
        }
        self.aliases.iter().any(|alias| normalized == *alias)
    }
}

/// The full section layout for one document type.
#[derive(Debug, Clone)]
pub struct DocumentSchema {
    /// The document type this schema describes.
    pub doc_type: DocumentType,
    /// Ordered list of canonical sections. Order reflects a typical reading
    /// sequence; importance is per-section, not positional.
    pub sections: &'static [SectionSchema],
}

impl DocumentSchema {
    /// Find the canonical section whose name or aliases match `heading`.
    ///
    /// Matching is case-insensitive and trims surrounding whitespace. Returns
    /// `None` when no section matches; callers should treat unmatched headings
    /// as unclassified content rather than an error.
    #[must_use]
    pub fn canonical_for(&self, heading: &str) -> Option<&SectionSchema> {
        self.sections.iter().find(|s| s.matches(heading))
    }
}

// ---------------------------------------------------------------------------
// Static schema definitions (one per DocumentType variant)
// ---------------------------------------------------------------------------

static RESUME_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "contact",
        aliases: &[
            "contact information",
            "contact info",
            "personal information",
            "personal details",
            "header",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "summary",
        aliases: &[
            "objective",
            "professional summary",
            "profile",
            "career objective",
            "about",
            "about me",
            "overview",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "skills",
        aliases: &[
            "technical skills",
            "core competencies",
            "competencies",
            "technologies",
            "tools",
            "key skills",
            "expertise",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "experience",
        aliases: &[
            "work experience",
            "professional experience",
            "employment",
            "employment history",
            "work history",
            "career history",
            "positions held",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "education",
        aliases: &[
            "academic background",
            "qualifications",
            "degrees",
            "academic history",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "projects",
        aliases: &[
            "personal projects",
            "side projects",
            "portfolio",
            "open source",
        ],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "certifications",
        aliases: &["certificates", "licenses", "accreditations", "credentials"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "publications",
        aliases: &["papers", "research", "articles", "journals"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "references",
        aliases: &["referees", "references available on request"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static RESEARCH_PAPER_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "abstract",
        aliases: &["summary", "synopsis"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "introduction",
        aliases: &["background", "motivation", "overview"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "related_work",
        aliases: &[
            "related work",
            "literature review",
            "prior work",
            "prior art",
            "background and related work",
        ],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "methodology",
        aliases: &[
            "methods",
            "approach",
            "experimental setup",
            "materials and methods",
            "experimental methodology",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "results",
        aliases: &[
            "experiments",
            "findings",
            "evaluation",
            "experimental results",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "discussion",
        aliases: &["analysis", "interpretation"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "conclusion",
        aliases: &[
            "conclusions",
            "concluding remarks",
            "summary and conclusion",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "references",
        aliases: &["bibliography", "works cited"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "appendix",
        aliases: &["appendices", "supplementary material", "supplemental"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static LEGAL_DOCUMENT_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "parties",
        aliases: &["contracting parties", "parties to the agreement"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "recitals",
        aliases: &[
            "background",
            "whereas",
            "preamble",
            "recitals and background",
        ],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "definitions",
        aliases: &["defined terms", "glossary"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "terms_and_conditions",
        aliases: &[
            "terms and conditions",
            "terms of service",
            "general terms",
            "agreement",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "obligations",
        aliases: &[
            "duties",
            "responsibilities",
            "covenants",
            "obligations of the parties",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "representations_and_warranties",
        aliases: &[
            "representations and warranties",
            "warranties",
            "representations",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "limitation_of_liability",
        aliases: &[
            "limitation of liability",
            "liability",
            "indemnification",
            "indemnity",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "termination",
        aliases: &["term and termination", "expiration"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "dispute_resolution",
        aliases: &[
            "dispute resolution",
            "arbitration",
            "governing law",
            "governing law and dispute resolution",
        ],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "miscellaneous",
        aliases: &["general provisions", "general", "miscellaneous provisions"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static TECHNICAL_MANUAL_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "overview",
        aliases: &["introduction", "about", "what is"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "prerequisites",
        aliases: &["requirements", "before you begin", "dependencies"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "installation",
        aliases: &["install", "setup", "getting started with installation"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "configuration",
        aliases: &["configure", "settings", "options", "environment variables"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "usage",
        aliases: &[
            "getting started",
            "quick start",
            "quickstart",
            "how to use",
            "tutorial",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "api_reference",
        aliases: &[
            "api reference",
            "api",
            "reference",
            "public api",
            "function reference",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "troubleshooting",
        aliases: &["faq", "common issues", "known issues", "debugging"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "glossary",
        aliases: &["terms", "definitions", "terminology"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "changelog",
        aliases: &["release notes", "what's new", "version history", "history"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static BUSINESS_REPORT_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "executive_summary",
        aliases: &["executive summary", "summary", "management summary"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "introduction",
        aliases: &["background", "context", "scope"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "methodology",
        aliases: &["approach", "research methodology", "data sources"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "findings",
        aliases: &["results", "key findings", "data", "analysis results"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "analysis",
        aliases: &["detailed analysis", "discussion", "deep dive"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "recommendations",
        aliases: &[
            "recommendations and next steps",
            "actions",
            "proposed actions",
        ],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "conclusion",
        aliases: &["conclusions", "closing remarks"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "appendix",
        aliases: &["appendices", "supporting data", "exhibits"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static MEETING_MINUTES_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "attendees",
        aliases: &[
            "participants",
            "present",
            "attendees list",
            "members present",
        ],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "agenda",
        aliases: &["topics", "agenda items"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "discussion",
        aliases: &["notes", "meeting notes", "discussion points", "minutes"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "action_items",
        aliases: &["action items", "actions", "tasks", "to-do", "todo"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "decisions",
        aliases: &["decisions made", "resolutions", "outcomes"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "next_steps",
        aliases: &["next steps", "follow-up", "follow up", "upcoming"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "next_meeting",
        aliases: &["next meeting", "next meeting date", "schedule"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static EMAIL_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "header",
        aliases: &["from", "to", "cc", "bcc", "date", "metadata"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "subject",
        aliases: &["subject line", "re", "fwd"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "body",
        aliases: &["message", "content", "text"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "attachments",
        aliases: &["attached", "files", "enclosures"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "signature",
        aliases: &["sign-off", "regards", "signed"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static BOOK_OR_CHAPTER_SECTIONS: &[SectionSchema] = &[
    SectionSchema {
        canonical_name: "preface",
        aliases: &["foreword", "preface and foreword", "prologue"],
        importance: SectionImportance::Medium,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "introduction",
        aliases: &["intro", "opening"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "chapter",
        aliases: &["part", "section"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "conclusion",
        aliases: &["epilogue", "afterword", "closing"],
        importance: SectionImportance::High,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "bibliography",
        aliases: &["references", "works cited", "further reading"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "index",
        aliases: &["subject index", "alphabetical index"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
    SectionSchema {
        canonical_name: "appendix",
        aliases: &["appendices", "supplementary material"],
        importance: SectionImportance::Low,
        expected_level: NodeLevel::Section,
    },
];

static GENERIC_SECTIONS: &[SectionSchema] = &[SectionSchema {
    canonical_name: "content",
    aliases: &["body", "main", "text"],
    importance: SectionImportance::High,
    expected_level: NodeLevel::Section,
}];

// ---------------------------------------------------------------------------
// Static DocumentSchema instances
// ---------------------------------------------------------------------------

static RESUME_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::Resume,
    sections: RESUME_SECTIONS,
};

static RESEARCH_PAPER_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::ResearchPaper,
    sections: RESEARCH_PAPER_SECTIONS,
};

static LEGAL_DOCUMENT_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::LegalDocument,
    sections: LEGAL_DOCUMENT_SECTIONS,
};

static TECHNICAL_MANUAL_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::TechnicalManual,
    sections: TECHNICAL_MANUAL_SECTIONS,
};

static BUSINESS_REPORT_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::BusinessReport,
    sections: BUSINESS_REPORT_SECTIONS,
};

static MEETING_MINUTES_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::MeetingMinutes,
    sections: MEETING_MINUTES_SECTIONS,
};

static EMAIL_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::Email,
    sections: EMAIL_SECTIONS,
};

static BOOK_OR_CHAPTER_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::BookOrChapter,
    sections: BOOK_OR_CHAPTER_SECTIONS,
};

static GENERIC_SCHEMA: DocumentSchema = DocumentSchema {
    doc_type: DocumentType::Generic,
    sections: GENERIC_SECTIONS,
};

/// Return the static [`DocumentSchema`] for a given [`DocumentType`].
///
/// Every variant has a schema; the `Generic` schema contains a single
/// catch-all `content` section.
#[must_use]
pub fn document_schema_for(doc_type: DocumentType) -> &'static DocumentSchema {
    match doc_type {
        DocumentType::Resume => &RESUME_SCHEMA,
        DocumentType::ResearchPaper => &RESEARCH_PAPER_SCHEMA,
        DocumentType::LegalDocument => &LEGAL_DOCUMENT_SCHEMA,
        DocumentType::TechnicalManual => &TECHNICAL_MANUAL_SCHEMA,
        DocumentType::BusinessReport => &BUSINESS_REPORT_SCHEMA,
        DocumentType::MeetingMinutes => &MEETING_MINUTES_SCHEMA,
        DocumentType::Email => &EMAIL_SCHEMA,
        DocumentType::BookOrChapter => &BOOK_OR_CHAPTER_SCHEMA,
        DocumentType::Generic => &GENERIC_SCHEMA,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_canonical_for_exact_match() {
        let schema = document_schema_for(DocumentType::Resume);
        let sec = schema.canonical_for("skills").unwrap();
        assert_eq!(sec.canonical_name, "skills");
        assert_eq!(sec.importance, SectionImportance::High);
    }

    #[test]
    fn resume_canonical_for_alias_case_insensitive() {
        let schema = document_schema_for(DocumentType::Resume);
        let sec = schema.canonical_for("Work Experience").unwrap();
        assert_eq!(sec.canonical_name, "experience");
    }

    #[test]
    fn resume_canonical_for_alias_with_whitespace() {
        let schema = document_schema_for(DocumentType::Resume);
        let sec = schema.canonical_for("  Professional Experience  ").unwrap();
        assert_eq!(sec.canonical_name, "experience");
    }

    #[test]
    fn resume_canonical_for_unknown_heading_returns_none() {
        let schema = document_schema_for(DocumentType::Resume);
        assert!(schema.canonical_for("random heading xyz").is_none());
    }

    #[test]
    fn research_paper_abstract_matches() {
        let schema = document_schema_for(DocumentType::ResearchPaper);
        let sec = schema.canonical_for("Abstract").unwrap();
        assert_eq!(sec.canonical_name, "abstract");
        assert_eq!(sec.importance, SectionImportance::High);
    }

    #[test]
    fn research_paper_methodology_alias() {
        let schema = document_schema_for(DocumentType::ResearchPaper);
        let sec = schema.canonical_for("Materials and Methods").unwrap();
        assert_eq!(sec.canonical_name, "methodology");
    }

    #[test]
    fn legal_definitions_match() {
        let schema = document_schema_for(DocumentType::LegalDocument);
        let sec = schema.canonical_for("Defined Terms").unwrap();
        assert_eq!(sec.canonical_name, "definitions");
    }

    #[test]
    fn technical_manual_api_reference_alias() {
        let schema = document_schema_for(DocumentType::TechnicalManual);
        let sec = schema.canonical_for("API").unwrap();
        assert_eq!(sec.canonical_name, "api_reference");
    }

    #[test]
    fn business_report_executive_summary_alias() {
        let schema = document_schema_for(DocumentType::BusinessReport);
        let sec = schema.canonical_for("Executive Summary").unwrap();
        assert_eq!(sec.canonical_name, "executive_summary");
    }

    #[test]
    fn meeting_minutes_action_items_alias() {
        let schema = document_schema_for(DocumentType::MeetingMinutes);
        let sec = schema.canonical_for("todo").unwrap();
        assert_eq!(sec.canonical_name, "action_items");
    }

    #[test]
    fn email_body_match() {
        let schema = document_schema_for(DocumentType::Email);
        let sec = schema.canonical_for("message").unwrap();
        assert_eq!(sec.canonical_name, "body");
    }

    #[test]
    fn book_conclusion_alias() {
        let schema = document_schema_for(DocumentType::BookOrChapter);
        let sec = schema.canonical_for("Epilogue").unwrap();
        assert_eq!(sec.canonical_name, "conclusion");
    }

    #[test]
    fn generic_content_match() {
        let schema = document_schema_for(DocumentType::Generic);
        let sec = schema.canonical_for("body").unwrap();
        assert_eq!(sec.canonical_name, "content");
    }

    #[test]
    fn all_document_types_have_schemas() {
        let types = [
            DocumentType::Resume,
            DocumentType::ResearchPaper,
            DocumentType::LegalDocument,
            DocumentType::TechnicalManual,
            DocumentType::BusinessReport,
            DocumentType::MeetingMinutes,
            DocumentType::Email,
            DocumentType::BookOrChapter,
            DocumentType::Generic,
        ];
        for dt in types {
            let schema = document_schema_for(dt);
            assert_eq!(schema.doc_type, dt);
            assert!(
                !schema.sections.is_empty(),
                "Schema for {dt:?} has no sections",
            );
        }
    }

    #[test]
    fn canonical_for_high_importance_sections_exist_in_all_schemas() {
        let types = [
            DocumentType::Resume,
            DocumentType::ResearchPaper,
            DocumentType::LegalDocument,
            DocumentType::TechnicalManual,
            DocumentType::BusinessReport,
            DocumentType::MeetingMinutes,
        ];
        for dt in types {
            let schema = document_schema_for(dt);
            let has_high = schema
                .sections
                .iter()
                .any(|s| s.importance == SectionImportance::High);
            assert!(
                has_high,
                "Schema for {dt:?} has no High-importance sections",
            );
        }
    }
}
