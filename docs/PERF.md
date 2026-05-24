# Pagebridge Ingest Performance Guide

This document describes how pagebridge 2.0+ (after the Phase I1-I9 ingest
performance overhaul) parallelizes ingest, how to tune it for your
provider/adapter combination, and what numbers to expect.

If you only read one paragraph: leave the defaults alone, and pagebridge will
do the right thing for free-tier Groq, Anthropic Haiku, OpenAI tier 1, and
local Ollama. The knobs below exist for paid tiers and unusual workloads.

## What changed (vs the v0.1 baseline)

| # | Phase | Bottleneck fixed | Speed-up |
|---|---|---|---|
| I1 | Concurrency-controlled summary fan-out | Sequential per-node LLM calls | 5-50x |
| I2 | Provider rate-limit declaration | Naive concurrency causing 429 cascades | (enables I1 safely) |
| I3 | Batched storage writes | One transaction per node upsert | 5-20x storage |
| I4 | Lazy tantivy commit scheduler | Per-node segment flushes (embedded only) | 3-10x embedded |
| I5 | Adaptive backoff and retry | No retry / no circuit on provider blips | Robustness |
| I6 | Progress API with real ETAs | Fire-and-forget background tasks | (UX) |
| I7 | Pre-flight summary cache lookup | Cache check inside the rate-limited worker | 2-5x on re-ingest |
| I8 | Skip-on-equivalent fast path | Identical re-ingest re-runs everything | Near-instant re-ingest |
| I9 | Tuning matrix and bench gate | Defaults from guesswork, no regression detection | (eng discipline) |

## The relevant knobs

```rust
PagebridgeOptions::new(storage, llm)
    .with_summary_worker_config(SummaryWorkerConfig {
        max_concurrency: 8,         // upper bound on in-flight LLM calls
        max_retries: 3,             // policy-level retries before giving up
        retry_backoff_ms: 500,      // initial backoff between retries
        timeout_per_task_ms: 60_000,// per-task hard timeout
    })
```

Each LLM provider declares its own rate limits, which clamp the effective
concurrency further:

```rust
LlmProvider::rate_limits() -> RateLimits {
    requests_per_minute: Option<u32>,
    tokens_per_minute: Option<u32>,
    max_concurrent_requests: Option<u32>,
}
```

The presets ship for free:

| Provider | Preset | RPM | TPM | Concurrent |
|---|---|---|---|---|
| Groq free | `RateLimits::groq_free()` | 30 | 15,000 | 4 |
| Groq paid | `RateLimits::groq_paid()` | 300 | 180,000 | 32 |
| Anthropic tier 1 | `RateLimits::anthropic_tier_1()` | 50 | 40,000 | 8 |
| OpenAI tier 1 | `RateLimits::openai_tier_1()` | 500 | 60,000 | 16 |
| Local (Ollama, llama.cpp) | `RateLimits::local()` | none | none | 2 |

Constructors apply the right preset automatically. Override with
`.with_rate_limits()`.

## Default tuning rationale

`SummaryWorkerConfig::default()` is `max_concurrency = 8`. From the
benchmark matrix:

- At 5ms mock-LLM latency, throughput plateaus around `max_concurrency=8`
  on a 100-leaf document. Going higher costs CPU contention with the
  BatchWriter for tiny gains.
- All major free-tier providers have `max_concurrent_requests <= 8`, so
  going higher than 8 just wastes scheduler permits.
- On the paid tiers (Groq paid, OpenAI tier 1), override to 16-32 and
  measure on your hardware.

## Recommended batch sizes (per adapter)

These are baked into `StorageAdapter::recommended_batch_size()`:

| Adapter | Recommended batch | Why |
|---|---|---|
| Memory | 1,000 | No I/O cost; bound to keep validation overhead bounded |
| JsonFile | 2,000 | Group by doc_id, one write per doc per batch |
| SQLite | 500 | Within SQLITE_MAX_VARIABLE_NUMBER (~999) safety margin |
| Postgres | 1,000 | Network round-trip cost amortized; PG can take more |
| MongoDB | 500 | Bulk replace_one has per-op overhead, 500 is the sweet spot |
| Embedded (redb+tantivy) | 1,000 | One redb txn + one collapsed tantivy commit per batch |

## Lazy tantivy commits (embedded adapter only)

```rust
EmbeddedAdapter::open_with_commit_config(
    path,
    CommitSchedulerConfig {
        max_dirty_docs: 500,            // commit after 500 dirty docs
        max_dirty_age: Duration::from_secs(2), // or every 2 seconds
    },
)
```

Search freshness is bounded by whichever threshold fires first. Call
`Pagebridge::flush()` to force a commit (ingest does this automatically
at the end of every document).

## Pareto frontier

The next 10% of throughput typically costs 50%+ more (rate limit pressure,
diminishing returns from extra concurrency, GC/contention). Examples:

- Groq free tier: doubling `max_concurrency` from 4 to 8 yields ~10% more
  throughput because RPM is the bottleneck, not concurrency. Spend the
  money on paid tier instead.
- Embedded adapter: dropping `max_dirty_age` from 2s to 200ms costs 5x
  more commits with no observable read-side benefit unless you query
  immediately after every write.

## Re-ingest performance

After Phase I7 + I8, re-ingest of an unchanged document is governed by:

1. `would_reingest_change(&params)` predicts the outcome cheaply (one
   `list_documents` + one structural parse).
2. `ingest_document_with_progress(params)` short-circuits when the
   `raw_text_hash` matches — returns under 100ms with zero LLM calls.
3. If `raw_text_hash` differs but `structural_hash` matches, only changed
   leaves' summaries are recomputed.

## CI bench gate

The criterion benches in `crates/pagebridge-core/benches/` are designed to
run on every PR (a nightly cron is sufficient). A 15%+ regression vs the
published baseline fails the gate.

Run locally:

```bash
cargo bench -p pagebridge-core --bench ingest_throughput
cargo bench -p pagebridge-core --bench ingest_parallel
cargo bench -p pagebridge-core --bench adapter_writes
```
