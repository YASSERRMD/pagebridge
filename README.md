<p align="center">
  <img src="docs/assets/banner.png" alt="pagebridge: cognitive retrieval for the database you already have." width="100%" />
</p>

<h1 align="center">pagebridge</h1>

<p align="center">
  <i>Vectorless, LLM-driven hierarchical retrieval, on the database you already have.</i>
</p>

<p align="center">
  <a href="https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml"><img src="https://github.com/YASSERRMD/pagebridge/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-1B2A4A.svg" alt="License: Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/rust-1.89%2B-C5A55A.svg" alt="Rust 1.89+" />
  <img src="https://img.shields.io/badge/python-3.9%2B-1B2A4A.svg" alt="Python 3.9+" />
  <img src="https://img.shields.io/badge/version-0.1.0-C5A55A.svg" alt="Version 0.1.0" />
</p>

---

## Why pagebridge

Most retrieval libraries assume you want a vector store. **`pagebridge` does not store, compute, or look up embeddings.** Instead, it builds a hierarchical tree of LLM-written summaries over your documents, persists that tree in **the database you already operate** (Postgres, SQLite, MongoDB, an embedded redb+tantivy store, or plain JSON files), and answers questions by letting an LLM walk that tree, guided by native BM25, until it finds the right leaves.

The result: **no embedding pipeline, no vector index to keep in sync, no separate similarity service**. Just your existing database, an LLM endpoint, and a deterministic explanation of every answer.

- One trait for storage (`StorageAdapter`), one for LLMs (`LlmProvider`).
- Two-pass ingestion: structure persists instantly, summaries fill in behind a content-hash cache.
- The LLM picks where to look. Every navigation step is recorded in a `QueryTrace` returned in-band on every `ask`.
- Async-first **Rust** API, async **Python** bindings, and a **`pagebridge`** CLI with `--json` output for piping.

### Ingest performance (Phase I1-I9 overhaul)

200-page PDF, end-to-end:

| Provider | Default tier | Wall time (target) |
|---|---|---|
| Groq (free tier, 30 RPM) | rate-limit bound | 90-150s |
| Groq (paid tier, 300 RPM) | concurrency 16 | 25-45s |
| Anthropic Haiku | tier 1 | 60-120s |
| OpenAI gpt-4o-mini | tier 1 | 60-90s |
| Local llama.cpp (M2 Pro) | compute bound | 180-300s |

Re-ingest of an identical document: **under 100ms with zero LLM calls.**
Cache-shared corpora (two docs with overlapping paragraphs): 60%+ hit rate
on the second ingest.

See [`docs/PERF.md`](docs/PERF.md) for the full tuning matrix.

---

## Architecture

<p align="center">
  <img src="docs/assets/architecture.png" alt="pagebridge end-to-end pipeline: Documents to Ingest to Summarize to Storage Adapter to Ask to Answer + Citations + Trace" width="100%" />
</p>

Every query goes through the same pipeline:

1. **Ingest** parses Markdown / PDF / plain text and builds a node tree.
2. **Summarize** runs a two-pass LLM summary over each node, cached by content hash.
3. **Storage adapter** persists the tree, raw chunks, and summaries, and exposes BM25 native to the backend.
4. **Ask** runs an LLM-guided beam navigator over the summary tree, scored by BM25, until it finds the most relevant leaves.
5. **Answer** is synthesised from those leaves and returned with citations and a complete trace.

For the deep dive, see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## Install

### Rust crate

Add `pagebridge` to your `Cargo.toml`, picking the **storage** and **LLM** feature flags you actually want. Nothing is enabled by default; you opt in.

```toml
[dependencies]
# Pick one (or more) storage adapters:
#   embedded · sqlite · postgres · mongodb · jsonfile
# Pick one (or more) LLM providers:
#   ollama · openai · anthropic
pagebridge = { version = "0.1", features = ["sqlite", "ollama"] }
tokio = { version = "1", features = ["full"] }
```

| Combo | Cargo features |
|---|---|
| SQLite + Ollama (laptop default) | `["sqlite", "ollama"]` |
| Embedded + Ollama (no external DB) | `["embedded", "ollama"]` |
| Postgres + OpenAI | `["postgres", "openai"]` |
| Mongo + Anthropic | `["mongodb", "anthropic"]` |
| All of it | `["embedded", "sqlite", "postgres", "mongodb", "jsonfile", "ollama", "openai", "anthropic"]` |

### Python package

The Python bindings ship as a maturin-built native extension named `pagebridge`. Requires Python ≥ 3.9 and a Rust toolchain (≥ 1.89) at build time.

**From PyPI (once released):**

```bash
pip install pagebridge
```

**From source (current path until the wheel is on PyPI):**

```bash
git clone https://github.com/YASSERRMD/pagebridge.git
cd pagebridge/crates/pagebridge-py

# Option A: develop into the current venv (fastest for iterating)
pip install maturin
maturin develop --release

# Option B: build a wheel and install it
maturin build --release
pip install ../../target/wheels/pagebridge-0.1.0-*.whl

# Verify
python -c "import pagebridge; print(pagebridge.__version__ if hasattr(pagebridge,'__version__') else 'ok')"
```

> The Python facade currently exposes `Pagebridge.open_sqlite(...)` and `Pagebridge.open_embedded(...)` paired with **Ollama** as the LLM. For OpenAI or Anthropic, use the Rust API or the CLI. Postgres/Mongo Python constructors are on the roadmap.

### CLI binary

```bash
# From crates.io (once released)
cargo install pagebridge-cli

# Or grab a prebuilt binary from the latest GitHub release.
# Or build from source:
git clone https://github.com/YASSERRMD/pagebridge.git
cd pagebridge
cargo build -p pagebridge-cli --release
./target/release/pagebridge --help
```

---

## Configure your LLM

`pagebridge` talks to LLMs over plain HTTP. No SDKs to install, no SaaS lock-in. Pick a provider, point at its endpoint, give it a model name (and an API key if needed). That's all.

| Provider | What you need | Default endpoint |
|---|---|---|
| **Ollama** | A running `ollama` server + a pulled model (e.g. `ollama pull qwen2.5:7b`) | `http://localhost:11434` |
| **OpenAI** | API key + model name (`gpt-4o-mini`, `gpt-4o`, …) | `https://api.openai.com` |
| **OpenAI-compatible** (vLLM, LM Studio, Together, Groq, Azure OpenAI, …) | Base URL + (optional) API key + model name | you supply |
| **Anthropic** | API key + model name (`claude-sonnet-4-5`, `claude-opus-4-5`, …) | `https://api.anthropic.com` |

### Rust: explicit LLM configuration

The umbrella crate exposes one-liner constructors that pair a storage adapter with an LLM. Use them when the defaults work for you:

```rust
use pagebridge::{sqlite_with_ollama, embedded_with_ollama,
                 postgres_with_openai, mongo_with_anthropic};

// 1. SQLite + Ollama running on localhost:11434
let bridge = sqlite_with_ollama("./demo.db", "qwen2.5:7b").await?;

// 2. Embedded redb+tantivy + Ollama
let bridge = embedded_with_ollama("./demo-embedded", "qwen2.5:7b").await?;

// 3. Postgres + OpenAI
let bridge = postgres_with_openai(
    "postgres://user:pass@localhost/pagebridge",
    &std::env::var("OPENAI_API_KEY")?,
    "gpt-4o-mini",
).await?;

// 4. MongoDB + Anthropic
let bridge = mongo_with_anthropic(
    "mongodb://localhost:27017",
    "pagebridge",
    &std::env::var("ANTHROPIC_API_KEY")?,
    "claude-sonnet-4-5",
).await?;
```

For anything bespoke (a custom Ollama host, an OpenAI-compatible endpoint, custom timeouts), assemble the storage adapter and LLM provider yourself and hand them to `Pagebridge::new`:

```rust
use std::sync::Arc;
use pagebridge::{Pagebridge, SqliteAdapter, OllamaProvider,
                 OpenAiCompatibleProvider, AnthropicProvider};

// Ollama on a different host
let storage = Arc::new(SqliteAdapter::open("./demo.db").await?);
let llm = Arc::new(OllamaProvider::new("http://gpu-box:11434", "qwen2.5:14b"));
let bridge = Pagebridge::new(storage, llm).await?;

// Any OpenAI-compatible endpoint (vLLM, Together, Groq, Azure …)
let llm = Arc::new(OpenAiCompatibleProvider::custom(
    "https://api.groq.com/openai",       // base_url
    Some(std::env::var("GROQ_API_KEY")?), // api_key
    "llama-3.3-70b-versatile",            // model
));

// LM Studio at its default localhost:1234
let llm = Arc::new(OpenAiCompatibleProvider::lm_studio("qwen2.5-7b-instruct"));

// vLLM
let llm = Arc::new(OpenAiCompatibleProvider::vllm(
    "http://localhost:8000", "meta-llama/Llama-3.1-8B-Instruct",
));

// Anthropic
let llm = Arc::new(AnthropicProvider::new(
    std::env::var("ANTHROPIC_API_KEY")?,
    "claude-sonnet-4-5",
));
```

### Python: explicit LLM configuration

The Python facade pairs SQLite or the embedded store with **Ollama**, with the URL and model as explicit kwargs:

```python
import asyncio, pagebridge

async def main():
    bridge = await pagebridge.Pagebridge.open_sqlite(
        "./demo.db",
        ollama_url="http://localhost:11434",   # default; override for a remote box
        model="qwen2.5:7b",                    # any model your Ollama server has pulled
    )
    # … or open_embedded with the same kwargs

asyncio.run(main())
```

### CLI: explicit LLM configuration

The CLI keeps its config in `~/.config/pagebridge/config.toml` (or `$PAGEBRIDGE_CONFIG`). Switch providers with `pagebridge config set`:

```bash
# Ollama (default)
pagebridge config set llm.provider ollama
pagebridge config set llm.base_url http://localhost:11434
pagebridge config set llm.model    qwen2.5:7b

# OpenAI
pagebridge config set llm.provider openai
pagebridge config set llm.api_key  "$OPENAI_API_KEY"
pagebridge config set llm.model    gpt-4o-mini

# Any OpenAI-compatible endpoint (Groq, Together, vLLM, LM Studio, Azure OpenAI …)
pagebridge config set llm.provider openai
pagebridge config set llm.base_url https://api.groq.com/openai
pagebridge config set llm.api_key  "$GROQ_API_KEY"
pagebridge config set llm.model    llama-3.3-70b-versatile

# Anthropic
pagebridge config set llm.provider anthropic
pagebridge config set llm.api_key  "$ANTHROPIC_API_KEY"
pagebridge config set llm.model    claude-sonnet-4-5

# Inspect what's set
pagebridge config show
```

For a deeper look at each provider (request shape, JSON-mode handling, retry behaviour), see [`docs/LLM_PROVIDERS.md`](docs/LLM_PROVIDERS.md).

---

## Quickstart

### Rust

```rust
use pagebridge::{sqlite_with_ollama, IngestParams, SourceKind};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Configure LLM here: SQLite for storage, Ollama serving `qwen2.5:7b`.
    // (Swap to embedded_with_ollama / postgres_with_openai / mongo_with_anthropic
    //  to change backends or providers; see "Configure your LLM" above.)
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

```python
import asyncio, pagebridge

async def main():
    # Configure LLM here. Defaults to Ollama at localhost:11434 with model qwen2.5:7b.
    # Override either kwarg to point at a different Ollama host or model.
    bridge = await pagebridge.Pagebridge.open_sqlite(
        "./demo.db",
        ollama_url="http://localhost:11434",
        model="qwen2.5:7b",
    )

    with open("samples/carbon-policy.md") as f:
        handle = await bridge.ingest_document(f.read(), title="Carbon Policy 2026")
    await bridge.wait_for_summaries(handle["doc_id"])

    ans = await bridge.ask("What is the implementation timeline?")
    print(ans["text"])
    for c in ans["citations"]:
        print(f"  - {c['section_title']} ({c['node_id']})")

asyncio.run(main())
```

### CLI

```bash
# 1. Initialise a SQLite store
pagebridge init sqlite --path ./demo.db

# 2. Configure the LLM (any provider; see "Configure your LLM" above)
pagebridge config set llm.provider ollama
pagebridge config set llm.base_url http://localhost:11434
pagebridge config set llm.model    qwen2.5:7b

# 3. Ingest and ask
pagebridge ingest samples/carbon-policy.md --title "Carbon Policy 2026"
pagebridge ask "What is the implementation timeline?"

# 4. Machine-readable mode (pipe to jq, etc.)
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

Anything OpenAI-compatible (Groq, Together, vLLM, LM Studio, Azure OpenAI) plugs into the `openai` provider; just change the base URL. See [`docs/LLM_PROVIDERS.md`](docs/LLM_PROVIDERS.md).

---

## How it compares

- **PageIndex**: same hierarchical, vectorless thesis. `pagebridge` adds a pluggable storage layer (the tree lives in your existing database, not in a sidecar JSON file), a pluggable LLM layer, async Rust, Python and CLI bindings, and first-class trace explainability.
- **ReasonDB / similar LLM-DBs**: focus on natural-language SQL over structured data. `pagebridge` focuses on retrieval over unstructured documents.
- **LlamaIndex / LangChain RAG**: vector-first by default. `pagebridge` deliberately occupies the no-vector lane: no embeddings, no ANN index, no embedding model to swap.

---

## Project layout

```
pagebridge/
├── crates/
│   ├── pagebridge-core/                  # Core types, traits, prompts, ingest, navigate, synthesize, trace
│   ├── pagebridge-adapter-embedded/      # redb + tantivy
│   ├── pagebridge-adapter-sqlite/        # SQLite + FTS5
│   ├── pagebridge-adapter-postgres/      # Postgres + tsvector
│   ├── pagebridge-adapter-mongodb/       # MongoDB + $text
│   ├── pagebridge-adapter-jsonfile/      # JSON file fallback
│   ├── pagebridge-llm-ollama/            # Ollama provider
│   ├── pagebridge-llm-openai/            # OpenAI-compatible provider
│   ├── pagebridge-llm-anthropic/         # Anthropic provider
│   ├── pagebridge/                       # Umbrella crate + convenience constructors
│   ├── pagebridge-py/                    # PyO3 async Python bindings (maturin)
│   └── pagebridge-cli/                   # `pagebridge` binary
├── docs/                                 # Architecture, adapters, providers, API, cookbook
│   └── assets/                           # Banner + architecture diagram
└── samples/                              # Demo documents
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

Licensed under the [Apache License, Version 2.0](LICENSE).

---

<p align="center">
  <sub>
    Crafted by <b>Mohamed Yasser</b> · Solutions Architect &nbsp;·&nbsp;
    <a href="https://github.com/YASSERRMD">@YASSERRMD</a>
  </sub>
</p>
