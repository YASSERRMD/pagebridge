# Storage adapters

`pagebridge` ships with five storage adapters in v0.1. They all implement the
same `StorageAdapter` trait, so swapping between them is a constructor change.

| Adapter   | Crate                              | Best for                       | BM25 backend          |
|-----------|------------------------------------|--------------------------------|-----------------------|
| Embedded  | `pagebridge-adapter-embedded`      | Single-binary apps, edge       | tantivy 0.22 BM25     |
| SQLite    | `pagebridge-adapter-sqlite`        | Local desktop, single-file ops | FTS5 BM25             |
| Postgres  | `pagebridge-adapter-postgres`      | Production / multi-writer      | tsvector + ts_rank_cd |
| MongoDB   | `pagebridge-adapter-mongodb`       | Document-DB shops              | $text + textScore     |
| JSON file | `pagebridge-adapter-jsonfile`      | Prototyping / migrations       | Substring fallback    |

## Embedded (redb + tantivy)

Layout under the directory you pass to `EmbeddedAdapter::open`:

```
pagebridge.redb           # nodes, docs, summary cache, raw offsets
pagebridge.tantivy/       # inverted index for BM25
raw/<doc_id>.bin          # append-only raw text per document
```

Strengths: zero external services, fast prefix scans, durable. Caveat: single
writer; concurrent ingests are serialized.

## SQLite

Standard sqlx + FTS5. WAL journaling, 5s busy timeout, 16-connection pool. FTS5
returns negative BM25 scores; the adapter negates them so the public API stays
"higher is more relevant" across backends.

Schema is created idempotently on `migrate`. Raw text is chunked into 256 KB
BLOBs keyed by `(doc_id, offset_start)`.

## Postgres

sqlx PgPool with 16 connections and a 10s acquire timeout. The nodes table has
a `tsvector` column maintained on every insert/upsert by weighted concatenation
of title (A), routing_summary (B), summary (C), and keywords (D). A GIN index
backs `ts_rank_cd` for ranking.

Raw text is chunked into 1 MB BYTEA rows. Summary cache uses BYTEA primary key.

## MongoDB

Uses the official `mongodb` crate. Indexes are created on first `migrate`:

- `pagebridge_nodes`: indexes on `doc_id`, `parent_id`, plus a compound text
  index on `title`/`routing_summary`/`summary`/`keywords`.
- `pagebridge_raw`: unique compound `(doc_id, offset_start)`.
- `pagebridge_summary_cache`: `_id` is the raw 32-byte source hash.

Search uses `$text` with `$meta: "textScore"` sort. Raw text chunked into a
dedicated collection (1 MB chunks). GridFS is intentionally avoided in v0.1.

## JSON file

Modeled on PageIndex's storage approach: one JSON file per document under
`trees/`, plus a global `index.json` and `summaries.json`. BM25 is approximated
by token-overlap substring scoring; documented in the rustdoc.

Use this adapter for prototyping, demos, or migrations into one of the
production adapters.

## Operational notes

- All adapters validate `NodeRecord` invariants on upsert.
- All BM25 implementations return positive, "higher is more relevant" scores.
- `delete_document` is transactional within the SQL adapters; for the embedded
  adapter it uses a single redb write transaction + a tantivy commit + a file
  removal.
