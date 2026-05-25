//! Versioned prompt library powering the cognitive query engine.
//!
//! The prompts in this module are the cognitive logic of the database. They
//! are the only place an LLM ever sees pagebridge's internal state, so changes
//! here are versioned and reviewed.
//!
//! The tiny templater supports two constructs:
//! - `{{name}}` for variable substitution.
//! - `{% for x in xs %}...{% endfor %}` for iteration over a list of context
//!   values. Inside the loop, the loop variable is accessible via `{{x}}` or
//!   field access like `{{x.title}}`.

use std::collections::HashMap;

use crate::error::{PagebridgeError, Result};
use crate::record::{NodeRecord, NodeSummary};
use crate::types::DocumentType;

/// Context passed into a prompt render. Holds every variable a template can
/// reference.
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    pub question: Option<String>,
    pub children: Vec<NodeSummary>,
    pub leaves: Vec<NodeRecord>,
    pub raw_text: Option<String>,
    pub document_title: Option<String>,
    pub current_path: Vec<NodeSummary>,
    /// Phase J5: document type used to select type-aware summarize prompts.
    pub document_type: Option<DocumentType>,
    /// Phase J5: canonical section name (e.g. "experience") for section-level
    /// summarization. When set together with `document_type`, the library
    /// picks the most specific prompt available.
    pub canonical_section: Option<String>,
}

/// Versioned bundle of prompt templates.
#[derive(Debug, Clone)]
pub struct PromptLibrary {
    version: u32,
    templates: HashMap<&'static str, &'static str>,
}

impl PromptLibrary {
    /// Returns the bundle of v1 prompts, including Phase J5 type-specific
    /// summarization prompts for resume and research paper sections.
    #[must_use]
    pub fn v1() -> Self {
        let mut templates: HashMap<&'static str, &'static str> = HashMap::new();
        templates.insert("navigate", NAVIGATE_V1);
        templates.insert("summarize", SUMMARIZE_V1);
        templates.insert("synthesize", SYNTHESIZE_V1);
        templates.insert("keywords", KEYWORDS_V1);
        // Phase J5: type-specific summarize prompts.
        // Resume sections.
        templates.insert(
            "summarize_resume_experience",
            SUMMARIZE_RESUME_EXPERIENCE_V1,
        );
        templates.insert("summarize_resume_skills", SUMMARIZE_RESUME_SKILLS_V1);
        templates.insert("summarize_resume_education", SUMMARIZE_RESUME_EDUCATION_V1);
        templates.insert("summarize_resume_summary", SUMMARIZE_RESUME_SUMMARY_V1);
        templates.insert(
            "summarize_resume_certifications",
            SUMMARIZE_RESUME_CERTIFICATIONS_V1,
        );
        // Research paper sections.
        templates.insert(
            "summarize_research_paper_abstract",
            SUMMARIZE_RESEARCH_PAPER_ABSTRACT_V1,
        );
        templates.insert(
            "summarize_research_paper_methodology",
            SUMMARIZE_RESEARCH_PAPER_METHODOLOGY_V1,
        );
        templates.insert(
            "summarize_research_paper_results",
            SUMMARIZE_RESEARCH_PAPER_RESULTS_V1,
        );
        templates.insert(
            "summarize_research_paper_conclusion",
            SUMMARIZE_RESEARCH_PAPER_CONCLUSION_V1,
        );
        // Legal document sections.
        templates.insert(
            "summarize_legal_document_obligations",
            SUMMARIZE_LEGAL_OBLIGATIONS_V1,
        );
        templates.insert(
            "summarize_legal_document_definitions",
            SUMMARIZE_LEGAL_DEFINITIONS_V1,
        );
        // Business report sections.
        templates.insert(
            "summarize_business_report_executive_summary",
            SUMMARIZE_BUSINESS_REPORT_EXEC_V1,
        );
        templates.insert(
            "summarize_business_report_recommendations",
            SUMMARIZE_BUSINESS_REPORT_RECS_V1,
        );
        // Meeting minutes sections.
        templates.insert(
            "summarize_meeting_minutes_action_items",
            SUMMARIZE_MEETING_ACTION_ITEMS_V1,
        );
        Self {
            version: 1,
            templates,
        }
    }

    /// Returns the prompt-library version number.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Render a prompt by name with the given context.
    pub fn render(&self, name: &str, ctx: &PromptContext) -> Result<String> {
        let tpl = self.templates.get(name).ok_or_else(|| {
            PagebridgeError::InvalidArgument(format!("unknown prompt name: {name}"))
        })?;
        render_template(tpl, ctx)
    }

    /// Phase J5: render the best summarize prompt available for the given
    /// context.
    ///
    /// Lookup priority (most specific to least):
    /// 1. `summarize_{doc_type}_{canonical_section}` (e.g. `summarize_resume_experience`)
    /// 2. `summarize_{doc_type}` (type-level fallback)
    /// 3. `summarize` (generic fallback, always present)
    pub fn render_summarize(&self, ctx: &PromptContext) -> Result<String> {
        // Try most specific: type + section.
        if let (Some(dt), Some(sec)) = (ctx.document_type, &ctx.canonical_section) {
            let key = format!("summarize_{}_{}", dt.as_str(), sec);
            if let Some(tpl) = self.templates.get(key.as_str()) {
                return render_template(tpl, ctx);
            }
        }
        // Try type-level fallback.
        if let Some(dt) = ctx.document_type {
            let key = format!("summarize_{}", dt.as_str());
            if let Some(tpl) = self.templates.get(key.as_str()) {
                return render_template(tpl, ctx);
            }
        }
        // Generic fallback always exists.
        self.render("summarize", ctx)
    }

    /// Return the prompt name that `render_summarize` would pick for a given
    /// document type and canonical section. Useful for tracing and tests.
    #[must_use]
    pub fn summarize_prompt_name(
        &self,
        doc_type: Option<DocumentType>,
        canonical_section: Option<&str>,
    ) -> &'static str {
        if let (Some(dt), Some(sec)) = (doc_type, canonical_section) {
            let key = format!("summarize_{}_{}", dt.as_str(), sec);
            if self.templates.contains_key(key.as_str()) {
                return self
                    .templates
                    .get_key_value(key.as_str())
                    .map_or("summarize", |(k, _)| *k);
            }
        }
        if let Some(dt) = doc_type {
            let key = format!("summarize_{}", dt.as_str());
            if self.templates.contains_key(key.as_str()) {
                return self
                    .templates
                    .get_key_value(key.as_str())
                    .map_or("summarize", |(k, _)| *k);
            }
        }
        "summarize"
    }

    /// JSON schema for the navigate prompt's response.
    #[must_use]
    pub fn navigate_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["descend", "select_leaves", "bm25_fallback", "widen"]
                },
                "node_id": { "type": "string" },
                "node_ids": { "type": "array", "items": { "type": "string" } },
                "query": { "type": "string" },
                "reason": { "type": "string" }
            }
        })
    }

    /// JSON schema for the summarize prompt's response.
    #[must_use]
    pub fn summarize_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["title", "routing_summary", "summary", "keywords"],
            "properties": {
                "title": { "type": "string" },
                "routing_summary": { "type": "string" },
                "summary": { "type": "string" },
                "keywords": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    /// JSON schema for the keywords prompt's response.
    #[must_use]
    pub fn keywords_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" }
        })
    }
}

const NAVIGATE_V1: &str = r#"You are a cognitive retrieval router for a document tree.

QUESTION:
{{question}}

CURRENT NODE: {{document_title}}
CURRENT PATH:{% for p in current_path %}
  - {{p.title}}{% endfor %}

CHILDREN AVAILABLE (id | title | level | routing summary):{% for c in children %}
  - {{c.node_id}} | {{c.title}} | {{c.level}} | {{c.routing_summary}}{% endfor %}

Decide the next move. Reply with a single JSON object using one of these actions:
- {"action":"descend","node_id":"<id of one child to descend into>"}
- {"action":"select_leaves","node_ids":["<leaf id>","<leaf id>"]}
- {"action":"bm25_fallback","query":"<rewritten search query>"}
- {"action":"widen","reason":"<why none of the children look promising>"}

Pick descend when one child clearly contains the answer.
Pick select_leaves when one or more children ARE the leaves to read.
Pick bm25_fallback when the tree shape is wrong and a keyword search would help.
Pick widen when the question is out of scope for this subtree.

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_V1: &str = r#"Summarize the following section for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Produce a JSON object with these fields:
- "title": a short, descriptive title (under 80 characters).
- "routing_summary": a single TOC-style line under 200 characters used to route navigation. State what the section is about, not what it contains.
- "summary": a 2 to 4 sentence chapter abstract that captures the key claims and entities.
- "keywords": up to 8 informative keywords, lowercase, no stopwords.

Respond ONLY with the JSON object, no prose.
"#;

const SYNTHESIZE_V1: &str = r"You are an explanatory assistant. Answer the user's question using ONLY the selected leaves.

QUESTION:
{{question}}

SELECTED LEAVES (id | title | text):{% for leaf in leaves %}
[{{leaf.node_id}}]
TITLE: {{leaf.title}}
TEXT:
{{leaf.summary}}{% endfor %}

Write a clear, direct answer grounded in the leaves above. Cite specific leaves inline by their id in square brackets, e.g. [doc:foo/sec:1/leaf:3]. End with a single line of the form:
<citations>doc:foo/sec:1/leaf:3, doc:foo/sec:2/leaf:1</citations>
listing every leaf id you actually used. Do not invent ids that are not in the list above.
";

const KEYWORDS_V1: &str = r"Extract up to 8 keywords from the following text. Return only a JSON array of lowercase strings, no prose, no explanations.

TEXT:
{{raw_text}}
";

// ---------------------------------------------------------------------------
// Phase J5: type-specific summarize prompt templates
// ---------------------------------------------------------------------------

const SUMMARIZE_RESUME_EXPERIENCE_V1: &str = r#"Summarize the work experience section of a resume for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: job titles, company names, employment dates, key responsibilities, and concrete achievements (metrics, promotions, projects delivered).

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters naming the employers and date range.
- "summary": 2 to 4 sentences capturing the progression of roles, key skills demonstrated, and notable outcomes.
- "keywords": up to 8 keywords (job titles, technologies, skills, company names).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESUME_SKILLS_V1: &str = r#"Summarize the skills section of a resume for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: technical skills, programming languages, frameworks, tools, and any proficiency levels mentioned.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters listing the main skill categories.
- "summary": 2 to 4 sentences capturing the breadth and depth of technical and non-technical skills.
- "keywords": up to 8 keywords drawn directly from the skill list.

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESUME_EDUCATION_V1: &str = r#"Summarize the education section of a resume for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: degrees, institutions, graduation years, majors/minors, GPA if stated, and relevant coursework or honors.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters naming the degree(s) and institution(s).
- "summary": 2 to 4 sentences covering the candidate's academic background and any notable achievements.
- "keywords": up to 8 keywords (degree names, institutions, fields of study).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESUME_SUMMARY_V1: &str = r#"Summarize the professional summary or objective of a resume for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: career goals, years of experience, primary domain, and the value the candidate offers.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters capturing the candidate's headline.
- "summary": 2 to 4 sentences that reproduce the key positioning claims from the professional summary.
- "keywords": up to 8 keywords (role names, domains, key skills mentioned).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESUME_CERTIFICATIONS_V1: &str = r#"Summarize the certifications section of a resume for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: certification names, issuing bodies, and dates obtained or expiry.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters listing the main certifications.
- "summary": 2 to 4 sentences covering the credentials and what they attest to.
- "keywords": up to 8 keywords (certification names, vendors, domains).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESEARCH_PAPER_ABSTRACT_V1: &str = r#"Summarize the abstract of a research paper for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the core research question, the approach taken, the key finding, and the claimed significance.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters capturing the paper's contribution.
- "summary": 2 to 4 sentences covering the research problem, method, result, and impact.
- "keywords": up to 8 keywords (problem domain, methods, datasets, metrics).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESEARCH_PAPER_METHODOLOGY_V1: &str = r#"Summarize the methodology section of a research paper for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the experimental design, datasets used, model architectures or algorithms, evaluation metrics, and baselines compared against.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters describing the experimental approach.
- "summary": 2 to 4 sentences covering the method, data, and evaluation setup.
- "keywords": up to 8 keywords (algorithms, datasets, metrics, techniques).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESEARCH_PAPER_RESULTS_V1: &str = r#"Summarize the results section of a research paper for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: quantitative outcomes, comparisons to baselines, ablation findings, and whether the hypothesis was supported.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters giving the headline result.
- "summary": 2 to 4 sentences covering the main metrics, comparisons, and statistical significance where stated.
- "keywords": up to 8 keywords (metric names, model names, benchmark names, key numeric results).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_RESEARCH_PAPER_CONCLUSION_V1: &str = r#"Summarize the conclusion of a research paper for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the main takeaway, limitations acknowledged, and future work directions.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters capturing the conclusion's message.
- "summary": 2 to 4 sentences on the contribution, its limitations, and the next research directions proposed.
- "keywords": up to 8 keywords (findings, limitations, future directions).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_LEGAL_OBLIGATIONS_V1: &str = r#"Summarize the obligations section of a legal document for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: what each party must do, deadlines, conditions, and consequences of non-compliance.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters naming the key obligations.
- "summary": 2 to 4 sentences covering who must do what, by when, and under what conditions.
- "keywords": up to 8 keywords (parties, obligations, deadlines, conditions).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_LEGAL_DEFINITIONS_V1: &str = r#"Summarize the definitions section of a legal document for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the defined terms and their meanings as used throughout the agreement.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters listing the key defined terms.
- "summary": 2 to 4 sentences describing the most important defined terms and their scope.
- "keywords": up to 8 keywords (the defined terms themselves).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_BUSINESS_REPORT_EXEC_V1: &str = r#"Summarize the executive summary of a business report for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the purpose of the report, the key findings, the recommended actions, and the business impact.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters stating the report's core message.
- "summary": 2 to 4 sentences covering the business context, key insight, and recommended next step.
- "keywords": up to 8 keywords (business domain, key metrics, recommended actions).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_BUSINESS_REPORT_RECS_V1: &str = r#"Summarize the recommendations section of a business report for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: the specific recommended actions, the rationale for each, and the expected outcomes or benefits.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters listing the top recommendations.
- "summary": 2 to 4 sentences covering the recommended actions, their priority, and expected impact.
- "keywords": up to 8 keywords (action items, business areas, expected outcomes).

Respond ONLY with the JSON object, no prose.
"#;

const SUMMARIZE_MEETING_ACTION_ITEMS_V1: &str = r#"Summarize the action items section of meeting minutes for a hierarchical retrieval index.

SECTION TITLE: {{document_title}}

RAW TEXT:
{{raw_text}}

Focus on: who is responsible for each action, what exactly must be done, and any stated deadlines.

Produce a JSON object with these fields:
- "title": a short descriptive title under 80 characters.
- "routing_summary": a single TOC-style line under 200 characters listing the key action items and owners.
- "summary": 2 to 4 sentences covering the assigned tasks, responsible parties, and due dates.
- "keywords": up to 8 keywords (assignees, tasks, deadlines).

Respond ONLY with the JSON object, no prose.
"#;

fn render_template(tpl: &str, ctx: &PromptContext) -> Result<String> {
    let mut out = String::with_capacity(tpl.len());
    let bytes = tpl.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'%' {
            // Block tag: {% ... %}.
            let end = find_tag_end(tpl, i + 2, "%}")?;
            let body = tpl[i + 2..end].trim();
            i = end + 2;
            if let Some(rest) = body.strip_prefix("for ") {
                let (loop_var, list_name) = parse_for(rest)?;
                let inner_end = find_block_end(tpl, i)?;
                let inner = &tpl[i..inner_end];
                let after = end_after_endfor(tpl, inner_end)?;
                render_loop(inner, &loop_var, &list_name, ctx, &mut out)?;
                i = after;
                continue;
            }
            return Err(PagebridgeError::InvalidArgument(format!(
                "unknown block tag: {body:?}"
            )));
        } else if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let end = find_tag_end(tpl, i + 2, "}}")?;
            let name = tpl[i + 2..end].trim();
            out.push_str(&resolve_var(name, ctx)?);
            i = end + 2;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

fn parse_for(rest: &str) -> Result<(String, String)> {
    let (var, list) = rest
        .split_once(" in ")
        .ok_or_else(|| PagebridgeError::InvalidArgument(format!("bad for tag: {rest:?}")))?;
    Ok((var.trim().to_owned(), list.trim().to_owned()))
}

fn find_tag_end(tpl: &str, start: usize, marker: &str) -> Result<usize> {
    tpl[start..]
        .find(marker)
        .map(|p| start + p)
        .ok_or_else(|| PagebridgeError::InvalidArgument(format!("unterminated tag: {marker}")))
}

fn find_block_end(tpl: &str, start: usize) -> Result<usize> {
    // Find the matching `{% endfor %}`. No nested for support in v1.
    tpl[start..]
        .find("{% endfor %}")
        .map(|p| start + p)
        .ok_or_else(|| PagebridgeError::InvalidArgument("missing endfor".into()))
}

fn end_after_endfor(tpl: &str, inner_end: usize) -> Result<usize> {
    let marker = "{% endfor %}";
    if tpl[inner_end..].starts_with(marker) {
        Ok(inner_end + marker.len())
    } else {
        Err(PagebridgeError::InvalidArgument("endfor not found".into()))
    }
}

fn render_loop(
    body: &str,
    loop_var: &str,
    list_name: &str,
    ctx: &PromptContext,
    out: &mut String,
) -> Result<()> {
    match list_name {
        "children" => {
            for c in &ctx.children {
                out.push_str(&render_inner_for_summary(body, loop_var, c)?);
            }
        }
        "leaves" => {
            for c in &ctx.leaves {
                out.push_str(&render_inner_for_record(body, loop_var, c)?);
            }
        }
        "current_path" => {
            for c in &ctx.current_path {
                out.push_str(&render_inner_for_summary(body, loop_var, c)?);
            }
        }
        other => {
            return Err(PagebridgeError::InvalidArgument(format!(
                "unknown loop list: {other}"
            )));
        }
    }
    Ok(())
}

fn render_inner_for_summary(body: &str, var: &str, item: &NodeSummary) -> Result<String> {
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    let b = body.as_bytes();
    while i < b.len() {
        if b[i] == b'{' && i + 1 < b.len() && b[i + 1] == b'{' {
            let end = find_tag_end(body, i + 2, "}}")?;
            let name = body[i + 2..end].trim();
            out.push_str(&resolve_summary_field(name, var, item)?);
            i = end + 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

fn render_inner_for_record(body: &str, var: &str, item: &NodeRecord) -> Result<String> {
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    let b = body.as_bytes();
    while i < b.len() {
        if b[i] == b'{' && i + 1 < b.len() && b[i + 1] == b'{' {
            let end = find_tag_end(body, i + 2, "}}")?;
            let name = body[i + 2..end].trim();
            out.push_str(&resolve_record_field(name, var, item)?);
            i = end + 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

fn resolve_var(name: &str, ctx: &PromptContext) -> Result<String> {
    match name {
        "question" => Ok(ctx.question.clone().unwrap_or_default()),
        "raw_text" => Ok(ctx.raw_text.clone().unwrap_or_default()),
        "document_title" => Ok(ctx.document_title.clone().unwrap_or_default()),
        other => Err(PagebridgeError::InvalidArgument(format!(
            "unknown variable: {other}"
        ))),
    }
}

fn resolve_summary_field(field: &str, var: &str, item: &NodeSummary) -> Result<String> {
    let (head, rest) = field.split_once('.').unwrap_or((field, ""));
    if head != var {
        return Err(PagebridgeError::InvalidArgument(format!(
            "loop var mismatch: expected {var}, got {head}"
        )));
    }
    Ok(match rest {
        "" | "title" => item.title.clone(),
        "node_id" => item.node_id.as_str().to_owned(),
        "routing_summary" => item.routing_summary.clone(),
        "level" => format!("{:?}", item.level).to_lowercase(),
        other => {
            return Err(PagebridgeError::InvalidArgument(format!(
                "unknown summary field: {other}"
            )));
        }
    })
}

fn resolve_record_field(field: &str, var: &str, item: &NodeRecord) -> Result<String> {
    let (head, rest) = field.split_once('.').unwrap_or((field, ""));
    if head != var {
        return Err(PagebridgeError::InvalidArgument(format!(
            "loop var mismatch: expected {var}, got {head}"
        )));
    }
    Ok(match rest {
        "" | "title" => item.title.clone(),
        "node_id" => item.node_id.as_str().to_owned(),
        "summary" => item.summary.clone(),
        "routing_summary" => item.routing_summary.clone(),
        "keywords" => item.keywords.join(", "),
        other => {
            return Err(PagebridgeError::InvalidArgument(format!(
                "unknown record field: {other}"
            )));
        }
    })
}
