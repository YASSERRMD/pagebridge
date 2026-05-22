# API reference (Rust)

The full Rust API for the umbrella `pagebridge` crate. The Python facade and
the CLI mirror this surface.

## Pagebridge

| Method | Description |
|--------|-------------|
| `Pagebridge::new(storage, llm)` | Minimal constructor. Calls `migrate` on the adapter. |
| `Pagebridge::new_with(opts)` | Construct from `PagebridgeOptions` with NavigationConfig and trace mode. |
| `ingest_document(params) -> DocumentHandle` | Two-pass ingest. Returns immediately after structural insert. |
| `wait_for_summaries(doc_id) -> ()` | Await the background summary task for one document. |
| `ask(question) -> Answer` | Cognitive query. Returns answer + citations + trace. |
| `ask_in_doc(doc_id, question) -> Answer` | Scoped to a single document. |
| `bm25_search(query, limit) -> Vec<SearchHit>` | Lower-level BM25 over leaves. |
| `navigate(question) -> Navigation` | Run navigation but skip synthesis. |
| `stats() -> PagebridgeStats` | Adapter + LLM counters. |
| `list_documents() -> Vec<DocumentEntry>` | All known documents. |
| `remove_document(doc_id) -> ()` | Delete a document and its nodes. |
| `storage()` / `llm()` / `prompts()` | Borrow the underlying handles. |

## IngestParams

```rust
pub struct IngestParams {
    pub title: String,
    pub source_kind: SourceKind,           // Markdown | Plain | Pdf
    pub raw_text: Vec<u8>,
    pub doc_id: Option<DocId>,
    pub user_metadata: BTreeMap<String, String>,
}
```

## Answer

```rust
pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
    pub trace: QueryTrace,
}

pub struct Citation {
    pub node_id: NodeId,
    pub doc_id: DocId,
    pub doc_title: String,
    pub section_title: String,
    pub page_range: Option<(u32, u32)>,
    pub excerpt: String,
}
```

## QueryTrace

Every `ask` produces a complete `QueryTrace`. The variants of `TraceStep`:

- `Bm25Candidates { count, top_score }`
- `NavigationDecision { node_id, action, reason, input_tokens, output_tokens, duration_ms }`
- `LeafSelection { leaves }`
- `SynthesisStart { leaf_count, total_chars }`
- `SynthesisDone { input_tokens, output_tokens, duration_ms }`
- `BudgetExhausted { reason }`

## NavigationConfig

```rust
pub struct NavigationConfig {
    pub max_depth: u8,                  // default 4
    pub beam_width: u8,                 // default 3
    pub bm25_candidate_limit: usize,    // default 30
    pub max_leaves: u8,                 // default 8
    pub max_llm_calls: u8,              // default 12
    pub token_budget_per_query: u32,    // default 32_000
}
```

## Python mirror

```python
import asyncio, pagebridge

async def main():
    b = await pagebridge.Pagebridge.open_sqlite("./demo.db", model="qwen2.5:7b")
    await b.ingest_document(open("policy.md").read(), title="Policy")
    ans = await b.ask("rollout timeline?")
    print(ans["text"])

asyncio.run(main())
```

## CLI mirror

```
pagebridge init sqlite --path ./demo.db
pagebridge config set llm.provider ollama
pagebridge config set llm.model qwen2.5:7b
pagebridge ingest policy.md --title "Policy"
pagebridge ask "rollout timeline?"
pagebridge list
pagebridge stats
```
