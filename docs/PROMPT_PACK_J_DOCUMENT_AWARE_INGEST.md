# Pagebridge Document-Aware Ingest: 9-Phase Prompt Pack (J1-J9)

**Project**: pagebridge
**Owner**: Mohamed Yasser (YASSERRMD / arafath.yasser@gmail.com)
**Position in roadmap**: After the I1-I9 Ingest Performance Overhaul. Recommended release tag: **v0.2.0**.
**Goal**: Fix the "I asked my CV about my skillsets and pagebridge couldn't find them" failure mode. Make pagebridge classify the document up front, choose a type-specific parser + summarization strategy + section taxonomy, and expand queries against canonical sections.

This is a **self-contained prompt pack**. Hand it to a fresh Claude session in the pagebridge repo and the model has everything it needs.

---

## Hard rules across all phases (carry-forward from earlier packs)

1. **Git identity**: every commit must use `user.name = "YASSERRMD"` and `user.email = "arafath.yasser@gmail.com"`. **Never** use "claud" or any AI identifier. **Never** add a `Co-Authored-By` trailer.
2. **Branching**: one branch per phase (`phase_J1`, `phase_J2`, ...). Push to that branch, open a PR, merge to main, delete the branch. **Never** merge or push directly to main.
3. **Atomic commits**: split each phase into many small commits, one logical change per commit, exactly like the I1-I9 pack did. Commit messages: `feat(scope): ...`, `test(scope): ...`, `docs(scope): ...`, `fix(scope): ...`, `chore(...)`, `bench(...)`.
4. **Style**: no em-dashes anywhere (commit messages, docs, code, comments). Prefer "explore" over "experience". Rust 1.89 edition 2021. Async-first. `clippy::pedantic` clean. No `unwrap()` in library code. Apache-2.0.
5. **CI gates that must stay green** (`.github/workflows/ci.yml` enforces them):
   - `cargo fmt --all -- --check`
   - `RUSTFLAGS="-D warnings" cargo build --workspace --all-targets --exclude pagebridge-py`
   - `cargo clippy --workspace --all-targets --exclude pagebridge-py -- -D warnings`
   - `cargo test --workspace --exclude pagebridge-py` on ubuntu/macos/windows
6. **Format after every code change**: run `cargo fmt --all` before every commit. New modules need a `#![allow(...)]` block matching the codebase convention (see `crates/pagebridge-core/src/ingest/mod.rs` for the canonical list).
7. **Existing `DocumentEntry` and bincode compat**: `DocumentEntry` is bincode-serialized by the embedded adapter. Any new fields must be `#[serde(default)]` (NOT `skip_serializing_if`); otherwise the embedded adapter's `list_documents` will fail with "unexpected end of file".
8. **Single-node upsert keeps v1 contract**: `EmbeddedAdapter::upsert_node` commits synchronously. Lazy commits only apply to the explicit `upsert_nodes_batch` path. Do not regress this.

---

## The failure mode (with evidence)

**User report**: "I ingested my CV (PDF) and asked 'what are my skillsets?' pagebridge could not answer."

**Why it fails today**:

1. `crates/pagebridge-core/src/ingest/pdf.rs` extracts flat text with `pdf-extract`, splits on form-feed (page break), and treats each *page* as a section. A one-page CV becomes one Section node with N paragraph leaves. There is no "Skills" / "Experience" / "Education" structure.
2. `crates/pagebridge-core/src/ingest/plain.rs` chunks by sentences. The "Skills" header line ends up in a leaf alongside the actual skill bullets, but the leaf's title is something generic like "Page 1: John Doe Software Engineer".
3. The summarizer (`crates/pagebridge-core/src/ingest/mod.rs::summarize_one_node`) uses a generic "summarize this" prompt. It writes a generic chapter abstract. It does not extract a "skills" facet.
4. `bm25_search` for the user's query "skillsets" looks for the literal stem "skillset". Most CVs use **"Skills"**, **"Technical Competencies"**, **"Expertise"**, **"Proficiencies"** etc. The stem differs. Zero hits.
5. Navigation gets no useful candidates, synthesis declines.

**The fundamental missing capability**: pagebridge ingests every document the same way regardless of what kind of document it is. CVs, research papers, contracts, manuals, and meeting minutes each have very different canonical structures and very different question patterns. A retrieval system that ignores document type is paying a recall tax on every query.

---

## What the fix looks like

Nine phases (J1-J9). Each builds on the previous.

| # | Phase | What it adds |
|---|---|---|
| J1 | Document type classifier | First-page LLM peek, classify into a canonical taxonomy. |
| J2 | Type-specific section schemas | Per-type expected sections + aliases (Skills <=> Competencies <=> Expertise). |
| J3 | Canonical section tagging | NodeRecord gains `canonical_section` and `aliases`. |
| J4 | Type-aware structural parsers | CV parser detects skill blocks even without proper headings. |
| J5 | Type-aware summary prompts | CV summaries enumerate skills; paper summaries expose abstract/methods. |
| J6 | Query intent expansion | Map user query to canonical section keys, expand BM25 over aliases. |
| J7 | Section-first navigation | When intent is clear, prefer the canonically-tagged subtree. |
| J8 | Evaluation harness | Test corpus + per-type question battery + recall@1 / answer-quality metrics. |
| J9 | Optional vision-mode first-page peek | For PDFs, rasterize first page so vision LLMs can classify and outline. |

After all nine, "what are my skillsets?" on a CV PDF returns the right leaves on the first try.

---

## Phase J1: Document Type Classifier

### Goal

When ingestion starts, peek at the first ~2000 bytes / first page, ask the configured LLM to classify the document into one of:

```rust
pub enum DocumentType {
    Resume,        // CV / resume / curriculum vitae
    ResearchPaper, // academic paper, arxiv preprint
    LegalDocument, // contract, terms of service, policy
    TechnicalManual, // SDK docs, runbook, user guide
    BusinessReport, // quarterly report, market analysis, white paper
    MeetingMinutes,
    Email,         // single email or thread
    BookOrChapter, // long-form prose
    Generic,       // none of the above
}
```

The classifier's output is stored on `DocumentEntry::document_type` and reused by all downstream phases (parsing, summarization, querying).

### Tasks

1. Add the enum + serde derives to `crates/pagebridge-core/src/types.rs`. Add `document_type: Option<DocumentType>` to `DocumentEntry` as `#[serde(default)]` (NOT `skip_serializing_if` -- bincode breaks otherwise; see hard rules).
2. New module `crates/pagebridge-core/src/ingest/classify.rs`:
   ```rust
   pub async fn classify_document(
       llm: &Arc<dyn LlmProvider>,
       prompts: &Arc<PromptLibrary>,
       title: &str,
       sample_text: &str,
   ) -> Result<(DocumentType, f32)> // (kind, confidence 0..1)
   ```
   Build a `complete_json` call with a schema like `{"document_type":"resume","confidence":0.92,"reasons":["..."]}`. Cap sample_text at 4000 chars.
3. Wire `classify_document` into `ingest_with_progress` BEFORE `build_structural`. Pass the result down so parsers can branch on type.
4. Update `DocumentEntry` construction in `ingest_with_progress` to persist the classified type.
5. Update **every** `DocumentEntry { ... }` literal across the workspace to include the new field. Use the Python bulk-patch trick from the I8 commit if there are too many. Adapters: postgres, sqlite, mongodb, mssql, mysql, oracle, jsonfile, embedded, tests for each.
6. Add a `ClassifyConfig` knob on `PagebridgeOptions`: `enabled: bool` (default true), `min_confidence: f32` (default 0.5; below this falls back to `Generic`), `sample_chars: usize` (default 4000).
7. Tests:
   - Mock LLM returns `{"document_type":"resume","confidence":0.9}` for a CV-shaped input; assert classify_document returns `(Resume, 0.9)`.
   - When confidence is below `min_confidence`, falls back to `Generic`.
   - Classifier failure (provider error) does NOT abort ingest; we proceed with `Generic` and log a warning.

### Acceptance

- `cargo test -p pagebridge-core --test classify` passes.
- `cargo clippy --workspace --all-targets --exclude pagebridge-py -- -D warnings` clean.
- New field round-trips through every adapter (extend `proptest_adapter.rs` if it exists).

### Atomic commits

1. `feat(types): DocumentType enum and DocumentEntry field`
2. `feat(ingest): classify module with provider-driven JSON output`
3. `feat(ingest): wire classifier into ingest_with_progress`
4. `feat(facade): ClassifyConfig on PagebridgeOptions`
5. `feat(adapter-*): back-fill document_type=None on list_documents (one commit per adapter)`
6. `test(classify): mock LLM happy path + low-confidence fallback + provider error`

---

## Phase J2: Type-Specific Section Schemas

### Goal

For each `DocumentType` declare a canonical schema: the sections users will ask about and the surface-form aliases each section appears under in real documents.

```rust
pub struct SectionSchema {
    pub canonical: &'static str,    // "skills"
    pub display: &'static str,      // "Skills"
    pub aliases: &'static [&'static str],
    pub priority: u8,               // 0..255; lower = root-level
    pub required: bool,             // if true, ingester warns when missing
}

pub struct DocumentSchema {
    pub document_type: DocumentType,
    pub sections: &'static [SectionSchema],
}
```

Example schemas:

**Resume**:
```rust
SectionSchema { canonical: "contact",    aliases: &["contact information","personal details","details"], priority: 0, required: true },
SectionSchema { canonical: "summary",    aliases: &["objective","profile","about","executive summary","professional summary"], priority: 1, required: false },
SectionSchema { canonical: "skills",     aliases: &["technical skills","skills","skill set","skillset","skillsets","competencies","technical competencies","proficiencies","expertise","areas of expertise","technologies","tech stack","tools"], priority: 2, required: true },
SectionSchema { canonical: "experience", aliases: &["work experience","professional experience","employment history","work history","employment","career"], priority: 3, required: true },
SectionSchema { canonical: "education",  aliases: &["education","academic background","qualifications","academics"], priority: 4, required: false },
SectionSchema { canonical: "projects",   aliases: &["projects","selected projects","personal projects","open source"], priority: 5, required: false },
SectionSchema { canonical: "certifications", aliases: &["certifications","certificates","licenses"], priority: 6, required: false },
SectionSchema { canonical: "languages",  aliases: &["languages","language proficiency","spoken languages"], priority: 7, required: false },
SectionSchema { canonical: "awards",     aliases: &["awards","honors","achievements","recognitions"], priority: 8, required: false },
SectionSchema { canonical: "publications", aliases: &["publications","papers","articles"], priority: 9, required: false },
```

**ResearchPaper**: title, abstract, introduction, background, related work, methods, results, discussion, conclusion, references, acknowledgements.

**LegalDocument**: parties, definitions, term, scope, obligations, payment, confidentiality, indemnification, termination, governing law, signatures.

**TechnicalManual**: overview, requirements, installation, quickstart, configuration, api reference, examples, troubleshooting, faq, changelog.

**BusinessReport**: executive summary, key findings, market analysis, financial summary, recommendations, appendix.

**MeetingMinutes**: attendees, agenda, discussion, action items, decisions, next meeting.

**Email**: header, salutation, body, signature, attachments.

**BookOrChapter** and **Generic**: empty schema; we treat them like the current pipeline.

### Tasks

1. New crate-internal module `crates/pagebridge-core/src/ingest/schema.rs` holding the schemas as `const` data. Use `static` arrays and `&'static str` so the whole table costs zero allocations.
2. Function `schema_for(doc_type: DocumentType) -> &'static DocumentSchema`.
3. Function `canonical_for(alias: &str, schema: &DocumentSchema) -> Option<&'static SectionSchema>` -- case-insensitive, punctuation-tolerant match against the alias list.
4. Tests:
   - Every alias in every schema maps back to its canonical via `canonical_for`.
   - "Technical Skills" maps to "skills" in Resume schema.
   - "skillsets" maps to "skills".
   - Unknown alias returns `None`.

### Atomic commits

1. `feat(schema): SectionSchema and DocumentSchema types`
2. `feat(schema): Resume schema with skills aliases`
3. `feat(schema): ResearchPaper schema`
4. `feat(schema): LegalDocument schema`
5. `feat(schema): TechnicalManual schema`
6. `feat(schema): BusinessReport and MeetingMinutes schemas`
7. `feat(schema): Email and BookOrChapter and Generic schemas`
8. `feat(schema): canonical_for case-insensitive matcher`
9. `test(schema): every alias round-trips to its canonical`

---

## Phase J3: Canonical Section Tagging on NodeRecord

### Goal

Add `canonical_section: Option<String>` and `aliases: Vec<String>` to `NodeRecord`. Update every adapter to persist them. Update BM25 indexing so canonical and aliases are part of the searchable haystack.

### Tasks

1. Extend `crates/pagebridge-core/src/record.rs::NodeRecord`:
   ```rust
   #[serde(default)]
   pub canonical_section: Option<String>,
   #[serde(default)]
   pub aliases: Vec<String>,
   ```
   Both `#[serde(default)]` (NOT `skip_serializing_if`) -- bincode-stable layout.
2. Update every `NodeRecord { ... }` literal across the workspace (production code AND tests). Bulk-patch via Python regex if the count is high. There are ~30 places.
3. Update every adapter's `upsert_node` / `upsert_nodes_batch` to write the new columns. For SQL adapters, add a migration that adds two nullable columns. For mongodb, mongo just adds the keys. For embedded, redb is opaque so the bincode change is automatic; tantivy needs the canonical_section indexed under field "canonical" and aliases joined into the keywords field so BM25 hits them.
4. Update `bm25_search` in every adapter so the canonical and aliases are weighted: SQLite gets a `weight` for the canonical column equal to the existing title weight; tantivy uses `setweight` style boosts; postgres tsvector uses `setweight(..., 'A')`.
5. Tests:
   - Insert a NodeRecord with `canonical_section = Some("skills")` and `aliases = ["competencies", "expertise"]`.
   - `bm25_search("competencies", 5)` returns this node.
   - `bm25_search("skillset", 5)` returns this node (because aliases contain "skillset" if we put it there at ingest time).

### Atomic commits

1. `feat(record): canonical_section and aliases on NodeRecord`
2. `feat(adapter-sqlite): migration + column writes for canonical_section`
3. `feat(adapter-sqlite): BM25 weights canonical and aliases`
4. `feat(adapter-postgres): migration + tsvector weighting`
5. `feat(adapter-embedded): tantivy field for canonical + keyword fold-in for aliases`
6. `feat(adapter-mongo): canonical + aliases on the docs`
7. `feat(adapter-jsonfile): canonical persistence`
8. `feat(adapter-mysql,mssql,oracle): migrations + writes`
9. `test(adapter-*): bm25 finds nodes via aliases (one commit per adapter)`

---

## Phase J4: Type-Aware Structural Parsers

### Goal

For a CV PDF, even when the source has no headings, detect the canonical sections and emit one NodeRecord per section with `canonical_section` set.

### Tasks

1. New module `crates/pagebridge-core/src/ingest/resume.rs`:
   - `parse_resume(doc_id, title, raw_text) -> Result<Vec<NodeRecord>>`
   - Strategy:
     1. First pass: scan lines for known alias headers (case-insensitive, trim whitespace, allow "Skills:" or "SKILLS" or "Skills & Expertise"). Use the Resume schema from J2.
     2. When an alias-matching line is found, treat the lines until the next alias-matching line (or EOF) as that section's content.
     3. Emit a Section NodeRecord with `canonical_section = Some("skills")` and `aliases = vec!["skills", "competencies", "expertise"]` (the schema's alias list).
     4. Inside each section, run the existing sentence-chunker to produce Leaf nodes.
     5. If NO known aliases match (the resume is exotic), fall through to an LLM-driven outline pass: send the raw text + the Resume schema to the LLM and ask it to return a JSON object mapping `{canonical: [line_start, line_end]}`. Use this to slice the document.
2. Similar modules for the other document types:
   - `ingest/research_paper.rs` (detect "Abstract", "1. Introduction", "References")
   - `ingest/legal.rs` (detect numbered clauses, "ARTICLE", "Section X")
   - `ingest/manual.rs` (detect "Installation", "API Reference")
   - `ingest/report.rs` (detect "Executive Summary", "Key Findings")
   - `ingest/minutes.rs` (detect "Attendees", "Action Items")
3. The dispatcher in `ingest/mod.rs::build_structural` reads `document_type` from the params (or from the J1 classifier output) and routes to the right parser. Falls back to the existing generic parsers for `Generic` / `BookOrChapter`.
4. PDF support: `pdf.rs` currently does naive page-splitting. For typed PDFs, first extract all text, then route the **text** to the type-specific parser (which handles its own sectioning regardless of page breaks). Page numbers are attached to leaves via `page_start`/`page_end` by tracking which page each line was extracted from.
5. Tests:
   - Hand-written CV markdown with "Skills" heading: parse_resume produces a Skills section with `canonical_section = Some("skills")`.
   - Hand-written CV plain text with "EXPERTISE" line (uppercase, no colon): same outcome.
   - CV with no recognizable headings: LLM fallback hits, and the LLM returns the right ranges.
   - A non-resume document accidentally classified as Resume still parses to *something*; recoverable.

### Atomic commits

1. `feat(ingest): resume parser with heading detection`
2. `feat(ingest): resume LLM-fallback outline pass for exotic layouts`
3. `feat(ingest): research_paper parser`
4. `feat(ingest): legal parser`
5. `feat(ingest): manual parser`
6. `feat(ingest): report parser`
7. `feat(ingest): minutes parser`
8. `feat(ingest): dispatcher in build_structural`
9. `feat(ingest-pdf): route text through type-specific parser, preserve page numbers`
10. `test(ingest-resume): heading-based and LLM-fallback paths`

---

## Phase J5: Type-Aware Summary Prompts

### Goal

The existing prompt at `crates/pagebridge-core/src/prompts.rs` is generic. Replace with a router that picks a prompt per `(canonical_section, document_type)`.

For example, the Resume `skills` section gets a prompt like:

```
You are summarizing the SKILLS section of a resume.

INPUT:
{section_text}

Produce JSON:
{
  "routing_summary": "<one line listing the top 5 skills, comma-separated>",
  "summary": "<one paragraph naming every skill grouped by category (languages, frameworks, tools, soft skills)>",
  "keywords": ["<skill1>", "<skill2>", ...]
}

Ensure every named skill in the input appears verbatim somewhere in the summary or keywords.
```

This matters because:
1. The routing_summary is what the navigation LLM sees. If it says "John worked at Foo Corp", queries for skills will skip this node. If it says "Python, Rust, Kubernetes, SQL, Docker", queries about any of those skills will route correctly.
2. The keywords feed BM25 directly.

### Tasks

1. Restructure `crates/pagebridge-core/src/prompts.rs` so prompts are keyed by `(canonical_section, document_type)`. A `PromptLibrary` exposes:
   ```rust
   pub fn summarize_prompt(
       &self,
       document_type: DocumentType,
       canonical_section: Option<&str>,
   ) -> &PromptTemplate;
   ```
   Returns a specialized template when one exists, otherwise the generic template.
2. Author specialized templates for the high-value section/type combos:
   - Resume: skills, experience, education, projects, summary.
   - ResearchPaper: abstract, methods, results, conclusion.
   - LegalDocument: definitions, obligations, termination.
   - TechnicalManual: installation, configuration, api reference.
   - BusinessReport: executive summary, key findings.
3. Each specialized template enforces a "verbatim mention" rule for entity-bearing sections (skills, parties, methods): every named entity in the input must appear in the summary or keywords. This is the single biggest recall win.
4. Update `summarize_one_node` in `ingest/mod.rs` to look up the right prompt by passing `(doc_type, node.canonical_section.as_deref())`.
5. Tests:
   - Run summarize_one_node against a CV skills section. Assert the returned keywords contain every skill in the input (case-insensitive substring).
   - Run against a research paper abstract. Assert the routing_summary is a single sentence and the summary mentions methods/results/conclusion.

### Atomic commits

1. `feat(prompts): PromptLibrary keyed by (type, section)`
2. `feat(prompts): resume skills template with verbatim-mention rule`
3. `feat(prompts): resume experience template`
4. `feat(prompts): resume education + projects + summary templates`
5. `feat(prompts): research_paper templates`
6. `feat(prompts): legal templates`
7. `feat(prompts): manual templates`
8. `feat(prompts): report templates`
9. `feat(ingest): summarize_one_node picks prompt by (type, section)`
10. `test(prompts): verbatim-mention enforcement on skills/methods/parties`

---

## Phase J6: Query Intent Expansion

### Goal

When the user asks "what are my skillsets?", recognize the intent ("skills" canonical section) and bias retrieval toward nodes tagged with that canonical key.

### Tasks

1. New module `crates/pagebridge-core/src/search/intent.rs`:
   ```rust
   pub struct QueryIntent {
       pub document_type_hint: Option<DocumentType>,
       pub canonical_sections: Vec<String>, // ranked
       pub keywords: Vec<String>,           // BM25-ready expansion
   }
   pub async fn classify_query_intent(
       llm: &Arc<dyn LlmProvider>,
       prompts: &Arc<PromptLibrary>,
       question: &str,
       known_doc_types: &[DocumentType],
   ) -> Result<QueryIntent>;
   ```
2. Implementation: one `complete_json` call. Prompt: "User question: {q}. Possible document types in the corpus: {types}. Return JSON: {document_type_hint, canonical_sections (ordered most likely first), keywords (synonyms and morphological variants)}."
3. **Local fallback** for low-latency / cost-sensitive deployments: a static `Vec<(Regex, &'static str)>` mapping common question patterns to canonical sections. "what are my skills" / "skillsets" / "what can I do" / "technical strengths" all map to `["skills"]`. If LLM intent classification is disabled, use this.
4. New facade method `Pagebridge::ask_with_intent(question)` (or extend `ask` to call intent classification internally; gate behind `intent_classification: bool` in `PagebridgeOptions`, default true).
5. In `crates/pagebridge-core/src/search/candidates.rs`, when `QueryIntent::canonical_sections` is non-empty, **boost** BM25 scores for nodes whose `canonical_section` matches by a factor of 3.0. Use a per-adapter weighted query where possible (Postgres tsvector setweight, tantivy boost queries) so the boost is computed inside the index, not after the fact.
6. Tests:
   - intent.rs unit test: "what are my skills?" maps to `canonical_sections: ["skills"]`.
   - Local fallback covers the same case without an LLM call.
   - End-to-end: ingest a CV, ask "what are my skillsets?", assert the answer contains at least one skill from the source.

### Atomic commits

1. `feat(search): QueryIntent type`
2. `feat(search): LLM-driven intent classifier`
3. `feat(search): regex local fallback for common questions`
4. `feat(facade): wire intent into ask`
5. `feat(search): canonical_section boost in candidate scoring`
6. `feat(adapter-*): weighted boost queries per adapter`
7. `test(intent): cv skills query end-to-end`

---

## Phase J7: Section-First Navigation

### Goal

When intent classification says the user's question targets a specific canonical section, change the navigation strategy from "BM25 candidates -> LLM tree walk" to "find the canonical section node -> read its leaves directly".

### Tasks

1. Update `crates/pagebridge-core/src/search/navigate.rs`:
   - If `QueryIntent::canonical_sections` is non-empty, first try `adapter.find_node_by_canonical(doc_id, canonical)`. If found, use its descendants as the leaf set, bypassing BM25 entirely.
   - Otherwise fall through to the existing BM25 + LLM-guided navigator.
2. New `StorageAdapter` method:
   ```rust
   async fn find_node_by_canonical(
       &self,
       doc_id: Option<&DocId>,
       canonical: &str,
   ) -> Result<Vec<NodeRecord>>;
   ```
   Default impl: linear scan via `list_documents` + `children_records`. SQL/embedded adapters override with an indexed lookup.
3. Migration: add an index on `canonical_section` for SQL adapters.
4. Tests:
   - Ingest a CV. Ask "skills". Assert navigation hit the canonical Skills node WITHOUT calling the navigator LLM (count LLM invocations and assert 0 navigation-stage calls).
   - For Generic-type documents (no canonical), navigation falls through to the BM25 path unchanged.

### Atomic commits

1. `feat(adapter): find_node_by_canonical trait method`
2. `feat(adapter-*): indexed find_node_by_canonical (one commit per adapter)`
3. `feat(search): section-first navigation when intent has canonical`
4. `feat(search): fall-through to BM25 navigator when no canonical match`
5. `test(navigate): section-first hits without navigator LLM call`

---

## Phase J8: Evaluation Harness

### Goal

Without a regression suite, this work is fragile. Build a tiny eval that runs on every PR.

### Tasks

1. Add `crates/pagebridge-eval/datasets/` with synthetic corpora:
   - 5 CVs (different layouts: chronological, functional, hybrid, exotic).
   - 5 research papers (different fields).
   - 5 manuals.
   - 5 reports.
   - 5 legal docs.
2. For each doc type, a JSON file of canonical questions with expected canonical sections:
   ```json
   [
     {"q":"what are the candidate's technical skills","expect_section":"skills"},
     {"q":"where did they go to school","expect_section":"education"},
     ...
   ]
   ```
3. A new binary `pagebridge-eval-run` that:
   - For each (doc, question), runs ingest + ask.
   - Captures which canonical section the chosen leaves came from.
   - Reports `recall@1` (did the right section get chosen first?) and `answer_grounded` (does the answer text contain at least one verbatim entity from the source section?).
4. A make target / cargo alias `cargo run -p pagebridge-eval --bin pagebridge-eval-run` that prints a table.
5. CI: a new job in `.github/workflows/ci.yml` that runs the eval against a mock LLM (deterministic canned responses for each question) and fails if `recall@1 < 0.9`.

### Atomic commits

1. `feat(eval): dataset directory + 5 sample CVs`
2. `feat(eval): research papers + manuals + reports + legal docs`
3. `feat(eval): question batteries per doc type`
4. `feat(eval): pagebridge-eval-run binary`
5. `feat(eval): recall@1 and answer_grounded metrics`
6. `chore(ci): eval gate at recall@1 >= 0.9`

---

## Phase J9: Optional Vision-Mode First-Page Peek

### Goal

For PDFs with unusual layouts (multi-column resumes, infographic resumes, scanned documents), text extraction misses the structure. Use a vision-capable provider (Anthropic Claude, GPT-4o) to look at the rendered first page.

### Tasks

1. `crates/pagebridge-vision` already exists. Extend it with a `classify_and_outline(image_bytes) -> (DocumentType, Vec<(canonical, page_range)>)` helper that uses `LlmProvider::supports_vision()`.
2. Add a `vision_classify: bool` knob to `ClassifyConfig` (default false, opt-in because vision is expensive).
3. When enabled, the J1 classifier rasterizes page 1 to PNG (via `pdf-extract`'s rendering API or a feature-gated `pdfium-render` dep), calls the vision LLM, and uses its output to seed both the document type AND the outline.
4. The outline from vision is fed to J4's parsers as a hint so they can confirm or refine the boundaries.
5. Tests:
   - Mock vision LLM returns a CV outline. Ingest a PDF (use a tiny test PDF in the repo). Assert outline-driven parsing kicks in and produces the canonical Skills/Experience/Education sections.

### Atomic commits

1. `feat(vision): classify_and_outline helper`
2. `feat(ingest): vision_classify knob on ClassifyConfig`
3. `feat(ingest): rasterize page 1 when vision_classify is on`
4. `feat(ingest): merge vision outline into structural parse`
5. `test(vision): outline-driven CV parse with mock vision LLM`

---

## Phase J10: Final Acceptance Checklist

After all nine phases. Not coding work.

1. **Original failure resolved**: ingest a CV PDF, run `pagebridge ask "what are my skillsets?"` -- the answer must list at least 3 skills from the source.
2. **No regression on Generic documents**: existing Phase I tests still pass.
3. **Recall@1 >= 0.9** on the eval suite (per Phase J8).
4. **bincode-stable**: re-ingest of a document written by a pre-J pagebridge still works; new fields default to None/empty.
5. **CI green** on ubuntu/macos/windows with `-D warnings`.
6. **CHANGELOG entry** for v0.2.0 with the before/after numbers from the eval suite.
7. **README update**: brief "How pagebridge handles different document types" section pointing at PERF.md and a new SCHEMAS.md (auto-generated from the J2 schemas).

Only then tag `v0.2.0`.

---

## Carry-Forward Instructions for the New Session

If you are starting a fresh Claude session and were handed this document:

1. Clone or `cd` into the pagebridge repo.
2. Set git identity:
   ```bash
   git config user.name "YASSERRMD"
   git config user.email "arafath.yasser@gmail.com"
   ```
3. Read `docs/PERF.md`, `CHANGELOG.md`, and skim `crates/pagebridge-core/src/ingest/mod.rs`, `crates/pagebridge-core/src/facade.rs`, `crates/pagebridge-core/src/types.rs`, `crates/pagebridge-core/src/adapter.rs` to ground yourself.
4. Read the prior pack: `crates/pagebridge-core/src/ingest/worker.rs`, `crates/pagebridge-core/src/llm_policy.rs`, `crates/pagebridge-core/src/ingest/progress.rs` -- these show the patterns this pack must follow.
5. Run the existing test + clippy gates locally so you know what green looks like:
   ```bash
   cargo fmt --all -- --check
   RUSTFLAGS="-D warnings" cargo build --workspace --all-targets --exclude pagebridge-py
   cargo clippy --workspace --all-targets --exclude pagebridge-py -- -D warnings
   cargo test --workspace --exclude pagebridge-py
   ```
6. Start Phase J1. Create `phase_J1` branch, work through its atomic commit list, push, open a PR, merge with `gh pr merge --merge --delete-branch`. Return to main. Move to J2.
7. **After each phase merges, watch CI** until green:
   ```bash
   gh pr checks <PR_NUMBER> --watch
   ```
   If anything fails, fix on a follow-up branch named `fix_<phase>_<short_description>`. Never push fixes directly to main.
8. **Schema bulk-edits**: when you need to add a field to `DocumentEntry` or `NodeRecord`, expect to touch ~30 files. Use the Python regex pattern from the I8 commit (`python3 - <<'PY' ... PY`) to bulk-patch struct literals.
9. **CI exclusions still apply**: `pagebridge-py` is excluded from the cross-OS matrix; testcontainer-based adapter tests skip silently when Docker is unavailable.
10. **Style gotchas** that broke us before:
    - bincode does NOT honor `skip_serializing_if`. Use only `#[serde(default)]`.
    - Pedantic clippy hates `&self -> &str` when the str is static; either widen the lifetime to `&'static str` or add `clippy::unnecessary_literal_bound` to the file's allow block.
    - `cargo fmt` rewrites long `#![allow(...)]` blocks aggressively; expect re-formats after each bulk edit.
    - `as_bytes()` on a string literal trips `clippy::byte_str_literal`; use `b"..."` instead.

If anything in this pack feels wrong or stale by the time you read it, **trust the codebase over this document**. The repo's current state is ground truth.

---

**End of pack.** After Phase J9 ships and Phase J10 passes, tag v0.2.0.
