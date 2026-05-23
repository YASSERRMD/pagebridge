<p align="center">
  <img src="docs/assets/banner.png" alt="pagebridge — Cognitive retrieval for the database you already have." width="100%" />
</p>

<h1 align="center">pagebridge</h1>

<p align="center">
  <i>Vectorless, LLM-driven hierarchical retrieval — on the database you already have.</i>
</p>

<p align="center">
  <a href="https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml"><img src="https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-1B2A4A.svg" alt="License: MIT OR Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/rust-1.89%2B-C5A55A.svg" alt="Rust 1.89+" />
  <img src="https://img.shields.io/badge/python-3.9%2B-1B2A4A.svg" alt="Python 3.9+" />
  <img src="https://img.shields.io/badge/version-0.1.0-C5A55A.svg" alt="Version 0.1.0" />
</p>

---

## Why pagebridge

Most retrieval libraries assume you want a vector store. **`pagebridge` does not store, compute, or look up embeddings.** Instead, it builds a hierarchical tree of LLM-written summaries over your documents, persists that tree in **the database you already operate** (Postgres, SQLite, MongoDB, an embedded redb+tantivy store, or plain JSON files), and answers questions by letting an LLM walk that tree — guided by native BM25 — until it finds the right leaves.

The result: **no embedding pipeline, no vector index to keep in sync, no separate similarity service**. Just your existing database, an LLM endpoint, and a deterministic explanation of every answer.

- One trait for storage (`StorageAdapter`), one for LLMs (`LlmProvider`).
- Two-pass ingestion — structure persists instantly, summaries fill in behind a content-hash cache.
- The LLM picks where to look. Every navigation step is recorded in a `QueryTrace` returned in-band on every `ask`.
- Async-first **Rust** API, async **Python** bindings, and a **`pagebridge`** CLI with `--json` output for piping.

---

## Architecture

<p align="center">
  <img src="docs/assets/architecture.png" alt="pagebridge end-to-end pipeline — Documents → Ingest → Summarize → Storage Adapter → Ask → Answer + Citations + Trace" width="100%" />
</p>

Every query goes through the same pipeline:

1. **Ingest** parses Markdown / PDF / plain text and builds a node tree.
2. **Summarize** runs a two-pass LLM summary over each node, cached by content hash.
3. **Storage adapter** persists the tree, raw chunks, and summaries — and exposes BM25 native to the backend.
4. **Ask** runs an LLM-guided beam navigator over the summary tree, scored by BM25, until it finds the most relevant leaves.
5. **Answer** is synthesised from those leaves and returned with citations and a complete trace.

For the deep dive, see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## Quickstart

### Rust

```toml
[dependencies]
pagebridge = { version = "0.1", features = ["sqlite", "ollama"] }
tokio = { version = "1", features = ["full"] }
```

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
    for c in &answer.citations {
        println!("  - {} ({})", c.section_title, c.node_id);
    }
    Ok(())
}
```

### Python

```bash
pip install pagebridge
```

```python
import asyncio, pagebridge

async def main():
    b = await pagebridge.Pagebridge.open_sqlite("./demo.db", model="qwen2.5:7b")
    await b.ingest_document(
        open("samples/carbon-policy.md").read(),
        title="Carbon Policy 2026",
    )
    ans = await b.ask("What is the implementation timeline?")
    print(ans["text"])
    for c in ans["citations"]:
        print(f"  - {c['section_title']} ({c['node_id']})")

asyncio.run(main())
```

### CLI

```bash
cargo install pagebridge-cli
# or download a prebuilt binary from the latest release

pagebridge init sqlite --path ./demo.db
pagebridge config set llm.provider ollama
pagebridge config set llm.model qwen2.5:7b

pagebridge ingest samples/carbon-policy.md --title "Carbon Policy 2026"
pagebridge ask "What is the implementation timeline?"

# Machine-readable mode
pagebridge --json ask "What is the implementation timeline?"
```

---

## Storage adapters

| Backend     | BM25 source              | Production? | Cargo feature | Notes                                            |
|-------------|--------------------------|:-----------:|---------------|--------------------------------------------------|
| Embedded    | tantivy                  | yes         | `embedded`    | redb key/value + tantivy full-text. Zero deps.   |
| SQLite      | FTS5                     | yes         | `sqlite`      | Single-file. Great for laptops and small servers.|
| PostgreSQL  | `tsvector` + `ts_rank_cd`| yes         | `postgres`    | Tested against a real container via testcontainers.|
| MongoDB     | `$text` + `textScore`    | yes         | `mongodb`     | Compound text index over title + chunk content.  |
| JSON files  | substring (fallback)     | no          | `jsonfile`    | Trivial backend for demos and tests.             |

See [`docs/ADAPTERS.md`](docs/ADAPTERS.md) for schema, indexing, and connection details per backend.

## LLM providers

| Provider           | Endpoint                  | JSON mode                  | Cargo feature  |
|--------------------|---------------------------|----------------------------|----------------|
| Ollama             | `POST /api/chat`          | `format: "json"`           | `ollama`       |
| OpenAI-compatible  | `POST /v1/chat/completions` | `response_format: json_object` | `openai`   |
| Anthropic          | `POST /v1/messages`       | Tool-use forcing           | `anthropic`    |

Bring your own URL — anything OpenAI-compatible (Groq, Together, vLLM, LM Studio, Azure OpenAI) plugs into the `openai` provider. See [`docs/LLM_PROVIDERS.md`](docs/LLM_PROVIDERS.md).

---

## How it compares

- **PageIndex** — same hierarchical, vectorless thesis. `pagebridge` adds a pluggable storage layer (the tree lives in your existing database, not in a sidecar JSON file), a pluggable LLM layer, async Rust, Python and CLI bindings, and first-class trace explainability.
- **ReasonDB / similar LLM-DBs** — focus on natural-language SQL over structured data. `pagebridge` focuses on retrieval over unstructured documents.
- **LlamaIndex / LangChain RAG** — vector-first by default. `pagebridge` deliberately occupies the no-vector lane: no embeddings, no ANN index, no embedding model to swap.

---

## Project layout

```
pagebridge/
├── crates/
│   ├── pagebridge-core/         # Core types, traits, prompts, ingest, navigate, synthesize, trace
│   ├── pagebridge-adapter-embedded/  # redb + tantivy
│   ├── pagebridge-adapter-sqlite/    # SQLite + FTS5
│   ├── pagebridge-adapter-postgres/  # Postgres + tsvector
│   ├── pagebridge-adapter-mongodb/   # MongoDB + $text
│   ├── pagebridge-adapter-jsonfile/  # JSON file fallback
│   ├── pagebridge-llm-ollama/        # Ollama provider
│   ├── pagebridge-llm-openai/        # OpenAI-compatible provider
│   ├── pagebridge-llm-anthropic/     # Anthropic provider
│   ├── pagebridge/              # Umbrella crate + convenience constructors
│   ├── pagebridge-py/           # PyO3 async Python bindings
│   └── pagebridge-cli/          # `pagebridge` binary
├── docs/                        # Architecture, adapters, providers, API, cookbook
│   └── assets/                  # Banner + architecture diagram
└── samples/                     # Demo documents
```

---

## Documentation

| Doc | What it covers |
|-----|----------------|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | The two-pass ingest model, beam navigation, synthesis, and trace. |
| [`docs/ADAPTERS.md`](docs/ADAPTERS.md) | Schema, indexing, and configuration for every storage adapter. |
| [`docs/LLM_PROVIDERS.md`](docs/LLM_PROVIDERS.md) | Wiring Ollama, OpenAI-compatible endpoints, and Anthropic. |
| [`docs/API.md`](docs/API.md) | Full Rust + Python + CLI reference. |
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Ten worked examples across all backends and providers. |

---

## License

Dual licensed under your choice of:

- [MIT](LICENSE-MIT)
- [Apache-2.0](LICENSE-APACHE)

---

<p align="center">
  <sub>
    Crafted by <b>Mohamed Yasser</b> · Solutions Architect &nbsp;·&nbsp;
    <a href="https://github.com/YASSERRMD">@YASSERRMD</a>
  </sub>
</p>
