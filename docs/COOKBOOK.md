# Cookbook

Ten worked examples for common pagebridge tasks.

## 1. Five-minute quickstart (SQLite + Ollama)

```bash
ollama pull qwen2.5:7b   # if you don't already have a model
pagebridge init sqlite --path ./demo.db
pagebridge config set llm.provider ollama
pagebridge config set llm.model qwen2.5:7b
pagebridge ingest samples/carbon-policy.md --title "Carbon Policy 2026"
pagebridge ask "What is the implementation timeline?"
```

## 2. Production setup (Postgres + OpenAI)

```bash
export OPENAI_API_KEY=sk-...
pagebridge init postgres --url "postgres://user:pass@host/db"
pagebridge config set llm.provider openai
pagebridge config set llm.model gpt-4o-mini
pagebridge ingest policy.pdf --kind pdf --title "Quarterly Filing"
pagebridge ask "What is the late-fee clause?"
```

## 3. Edge deployment (embedded redb + tantivy)

```rust
let bridge = pagebridge::embedded_with_ollama("./data", "qwen2.5:7b").await?;
```

Ships as a single binary with a single data directory. Add the CLI to your
container image; mount the data directory as a volume.

## 4. Inside an Axum app (Rust)

```rust
let bridge = pagebridge::sqlite_with_ollama("./data.db", "qwen2.5:7b").await?;
let state = std::sync::Arc::new(bridge);
let app = axum::Router::new()
    .route("/ask", axum::routing::post({
        let state = state.clone();
        move |body: String| async move {
            let answer = state.ask(&body).await.unwrap();
            axum::Json(answer)
        }
    }));
```

## 5. Inside a Django app (Python)

```python
# views.py
import asyncio, pagebridge
_BRIDGE = None

async def get_bridge():
    global _BRIDGE
    if _BRIDGE is None:
        _BRIDGE = await pagebridge.Pagebridge.open_sqlite("./data.db")
    return _BRIDGE

async def ask_view(request):
    b = await get_bridge()
    ans = await b.ask(request.GET["q"])
    return JsonResponse(ans)
```

## 6. Migrating from PageIndex JSON files

Drop existing PageIndex JSON files into a directory, then point the JSON-file
adapter at it. Substring fallback works while you migrate; once you have a
real database wired up, re-ingest the same documents into the SQL or embedded
adapter.

```bash
pagebridge init jsonfile --path ./pageindex-export
pagebridge list
```

## 7. Ingesting PDFs in bulk

```bash
for f in pdfs/*.pdf; do
  pagebridge ingest "$f" --kind pdf --title "$(basename "$f" .pdf)"
done
```

The PDF parser uses `pdf-extract` to pull text page by page. Each page becomes
a Page-level node with leaf chunks under it.

## 8. Custom navigation policy

```rust
use pagebridge::{NavigationConfig, PagebridgeOptions};
let mut nav = NavigationConfig::default();
nav.max_depth = 6;
nav.max_leaves = 16;
nav.bm25_candidate_limit = 60;
let opts = PagebridgeOptions {
    storage, llm, navigation: nav, trace_storage: None, summary_model_fingerprint: None,
};
let bridge = pagebridge::Pagebridge::new_with(opts).await?;
```

## 9. Custom prompt overrides

The prompt library is exposed via `bridge.prompts()`. Today the v1 templates
live in `pagebridge_core::prompts`. If you want to override, construct your
own `PromptLibrary` in a fork of the crate; first-class user overrides land in
v0.2.

## 10. Trace-based debugging

Every `ask` returns a `QueryTrace`. Run with `--json` and inspect:

```bash
pagebridge --json ask "What is the timeline?" | jq '.trace.steps'
```

You will see the BM25 candidate count, every navigation action, the leaf
selection, and synthesis token counts.
