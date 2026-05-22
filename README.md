# pagebridge

> Cognitive retrieval for the database you already have.

[![CI](https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml/badge.svg)](https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

`pagebridge` is a vectorless, LLM-driven hierarchical retrieval library that
runs on top of an existing storage backend (PostgreSQL, SQLite, MongoDB, an
embedded redb+tantivy store, or plain JSON files) and uses any configured LLM
provider (Ollama, OpenAI-compatible, Anthropic). It does not compute or store
embeddings: BM25 plus an LLM-guided beam search over a tree of summaries is
the entire similarity story.

## Highlights

- One trait for storage (`StorageAdapter`), one for LLMs (`LlmProvider`).
- Two-pass ingestion: the structural insert returns immediately; summaries
  populate in the background with a content-hash cache.
- LLM picks where to look. Every step is recorded in a `QueryTrace`
  returned in-band on every `ask`.
- Async-first Rust API, async Python bindings, a CLI with `--json` output.

## Quickstart (Rust)

```rust
use pagebridge::{sqlite_with_ollama, IngestParams, SourceKind};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bridge = sqlite_with_ollama("./demo.db", "qwen2.5:7b").await?;
    let handle = bridge.ingest_document(IngestParams {
        title: "Carbon Policy 2026".into(),
        source_kind: SourceKind::Markdown,
        raw_text: std::fs::read("samples/carbon-policy.md")?,
        doc_id: None,
        user_metadata: Default::default(),
    }).await?;
    bridge.wait_for_summaries(&handle.doc_id).await?;
    let answer = bridge.ask("What is the implementation timeline?").await?;
    println!("{}", answer.text);
    for c in &answer.citations { println!("  - {} ({})", c.section_title, c.node_id); }
    Ok(())
}
```

## Quickstart (Python)

```python
import asyncio, pagebridge

async def main():
    b = await pagebridge.Pagebridge.open_sqlite("./demo.db", model="qwen2.5:7b")
    await b.ingest_document(open("samples/carbon-policy.md").read(),
                            title="Carbon Policy 2026")
    ans = await b.ask("What is the implementation timeline?")
    print(ans["text"])

asyncio.run(main())
```

## Quickstart (CLI)

```bash
pagebridge init sqlite --path ./demo.db
pagebridge config set llm.provider ollama
pagebridge config set llm.model qwen2.5:7b
pagebridge ingest samples/carbon-policy.md --title "Carbon Policy 2026"
pagebridge ask "What is the implementation timeline?"
```

## Feature matrix

| Backend       | BM25 source           | Production? | Cargo feature |
|---------------|-----------------------|-------------|---------------|
| Embedded      | tantivy               | yes         | `embedded`    |
| SQLite        | FTS5                  | yes         | `sqlite`      |
| Postgres      | tsvector + ts_rank_cd | yes         | `postgres`    |
| MongoDB       | $text + textScore     | yes         | `mongodb`     |
| JSON file     | substring (fallback)  | no          | `jsonfile`    |

| LLM provider       | Endpoint               | JSON mode               | Cargo feature |
|--------------------|------------------------|-------------------------|---------------|
| Ollama             | /api/chat              | `format: "json"`        | `ollama`      |
| OpenAI-compatible  | /v1/chat/completions   | `response_format: json` | `openai`      |
| Anthropic          | /v1/messages           | Tool-use forcing        | `anthropic`   |

## Comparison

- **PageIndex**: same hierarchical, vectorless thesis. `pagebridge` adds a
  pluggable storage layer (so the tree lives in your existing database, not
  a JSON file), a pluggable LLM layer, async Rust, Python and CLI bindings,
  and first-class trace explainability.
- **ReasonDB / similar LLM-DBs**: focus on natural-language SQL. `pagebridge`
  focuses on retrieval over unstructured documents.
- **LlamaIndex / LangChain RAG**: vector-first by default. `pagebridge`
  occupies the no-vector lane.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Storage adapters](docs/ADAPTERS.md)
- [LLM providers](docs/LLM_PROVIDERS.md)
- [API reference](docs/API.md)
- [Cookbook](docs/COOKBOOK.md)

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your
option.
