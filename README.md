# pagebridge

> Cognitive retrieval for the database you already have.

Status: **pre-alpha**. APIs, schemas, and behavior may change without notice until 0.1.0.

`pagebridge` is a vectorless, LLM-driven hierarchical retrieval library that runs on top of
Postgres, SQLite, MongoDB, an embedded redb+tantivy store, or plain JSON files. The LLM
provider is configured at the library level (Ollama, OpenAI-compatible, Anthropic). No
embeddings are involved at any point; BM25 (or the underlying database's native FTS) is
the only similarity primitive.

## Quickstart (Rust, planned)

```rust
use pagebridge::{sqlite_with_ollama, IngestParams, SourceKind};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bridge = sqlite_with_ollama("./demo.db", "qwen2.5:7b").await?;
    let handle = bridge.ingest_document(IngestParams {
        title: "Carbon Policy 2026".into(),
        source_kind: SourceKind::Markdown,
        raw_text: std::fs::read("policy.md")?,
        doc_id: None,
        user_metadata: Default::default(),
    }).await?;
    bridge.wait_for_summaries(&handle.doc_id).await?;
    let answer = bridge.ask("What is the implementation timeline?").await?;
    println!("{}", answer.text);
    Ok(())
}
```

## License

Dual licensed under either of MIT (`LICENSE-MIT`) or Apache-2.0 (`LICENSE-APACHE`), at
your option.
