# Architecture

`pagebridge` is a vectorless, LLM-driven hierarchical retrieval library. It runs
on top of an existing database (Postgres, SQLite, MongoDB, an embedded
redb+tantivy store, or plain JSON files) and uses any configured LLM provider
to do cognitive navigation over the document tree.

## Why vectorless

Vector retrieval has well-known weaknesses: hidden chunk boundaries, brittle
re-indexing on model upgrades, and limited explainability. `pagebridge` takes
the position that:

- BM25 (or the underlying database's native FTS) is sufficient for candidate
  selection over short structured spans.
- An LLM can navigate a hierarchical tree of summaries and pick the right
  leaves with very few calls when the index is shaped well.
- Storing only summaries plus raw byte offsets keeps the index lean and the
  retrieval explainable.

## The pieces

```
                                  ┌──────────────────────┐
                                  │  Ingest pipeline     │
   raw doc bytes ───────────────► │ markdown / plain /pdf│
                                  └──────────┬───────────┘
                                             │
                                  ┌──────────▼───────────┐
                                  │ NodeRecord tree      │
                                  │ (titles + spans)     │
                                  └──────────┬───────────┘
                                             │
                            ┌────────────────┴────────────────┐
                            │ StorageAdapter (per backend)    │
                            │ nodes, docs, raw, summary cache │
                            └────────────────┬────────────────┘
                                             │
                            ┌────────────────┴────────────────┐
                            │ Background summary task         │
                            │ (LLM-driven, cache-aware)       │
                            └────────────────┬────────────────┘
                                             │
                            ┌────────────────▼────────────────┐
                            │ ask(question)                   │
                            │ ┌──────────────┐ ┌───────────┐  │
                            │ │ BM25         │ │ Navigator │  │
                            │ │ candidates   │ │ (LLM beam)│  │
                            │ └──────────────┘ └─────┬─────┘  │
                            │                       │        │
                            │           ┌───────────▼─────┐  │
                            │           │ Synthesizer     │  │
                            │           │ + citations     │  │
                            │           └─────────────────┘  │
                            └─────────────────────────────────┘
```

## Core types

- `NodeId`: hierarchical id of the form `doc:<slug>/<kind>:<value>/<kind>:<value>...`.
  Lexicographic ordering ensures that all children of a parent are contiguous
  under any byte-ordered range scan.
- `NodeRecord`: the persisted node. Title, level (Document/Section/Subsection/
  Page/Leaf), routing summary, full summary, child ids, optional span into raw
  bytes, optional page range, keywords.
- `StorageAdapter`: the trait every backend implements. ~20 methods covering
  upserts, queries, BM25 search, raw text, summary cache, and stats.
- `LlmProvider`: the trait every LLM provider implements. Provides text and
  JSON completion plus token estimation.
- `Pagebridge`: the appliance facade. `ingest_document`, `ask`,
  `wait_for_summaries`, `list_documents`, `remove_document`, `stats`.

## Two-pass ingestion

`ingest_document` does the structural insert synchronously: the tree, raw text,
and document index land in storage right away. A background tokio task then
walks the tree bottom-up to fill in summaries via the LLM. A
content-hash-keyed cache short-circuits repeated work across re-ingestions of
the same material.

## Five core algorithms

1. **Tree construction**: source-specific (markdown headings, sentence chunks,
   per-page PDF chunks).
2. **BM25 candidate selection**: adapter-native; scores normalized to
   "higher is more relevant".
3. **LLM-guided beam search**: at each level, the LLM picks descend /
   select_leaves / bm25_fallback / widen from a JSON-constrained schema.
4. **Synthesis**: leaves are hydrated with raw text, fed to the LLM with a
   citation contract, parsed back into an `Answer` with `Citation` rows.
5. **Trace assembly**: every step (BM25, decisions, leaf selection, synthesis,
   budget) lands in a `QueryTrace` returned in-band on every `ask`.

## What pagebridge deliberately does NOT do

- It does not compute or store vector embeddings.
- It does not own the storage. Every adapter writes to a backend the user
  already runs.
- It does not stream tokens (v0.1).
- It does not provide a web UI (v0.1).
