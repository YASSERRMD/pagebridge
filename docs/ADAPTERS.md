# Storage adapters

`pagebridge` ships with eight storage adapters across v0.1 and v0.2. They all
implement the same `StorageAdapter` trait, so swapping between them is a
constructor change.

| Adapter    | Crate                              | Best for                       | BM25 backend                  |
|------------|------------------------------------|--------------------------------|-------------------------------|
| Embedded   | `pagebridge-adapter-embedded`      | Single-binary apps, edge       | tantivy 0.22 BM25             |
| SQLite     | `pagebridge-adapter-sqlite`        | Local desktop, single-file ops | FTS5 BM25                     |
| Postgres   | `pagebridge-adapter-postgres`      | Production / multi-writer      | tsvector + ts_rank_cd         |
| MySQL      | `pagebridge-adapter-mysql`         | LAMP-stack and MariaDB shops   | MATCH AGAINST natural language|
| MongoDB    | `pagebridge-adapter-mongodb`       | Document-DB shops              | $text + textScore             |
| SQL Server | `pagebridge-adapter-mssql`         | Enterprise / Windows shops     | Full-Text (LIKE fallback)     |
| Oracle     | `pagebridge-adapter-oracle`        | Enterprise / government shops  | Oracle Text (LIKE fallback)   |
| JSON file  | `pagebridge-adapter-jsonfile`      | Prototyping / migrations       | Substring fallback            |

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

## MySQL / MariaDB

`mysql_async::Pool` with up to 16 concurrent connections. The schema is
identical in shape to the Postgres adapter but uses native MySQL types:
`MEDIUMTEXT` for summaries, `JSON` for child id lists and keywords, and a
`FULLTEXT` index over `(title, routing_summary, summary)`. Search runs through
`MATCH(...) AGAINST(... IN NATURAL LANGUAGE MODE)`; the natural-language
relevance score is mapped directly into the public `SearchHit::score`.

Raw text is chunked into 8 MB `MEDIUMBLOB` rows keyed by
`(doc_id, offset_start)`. Upserts use `INSERT ... ON DUPLICATE KEY UPDATE`.

Compatibility: tested against MariaDB 10/11 in the integration tests; the
MySQL 8 protocol is also supported via the same connection URL.

## SQL Server

`tiberius` over a `bb8` connection pool, up to 16 connections. The
adapter accepts either an ADO.NET-style connection string
(`MSSqlAdapter::from_ado_string`) or a fully-formed `tiberius::Config`
(`MSSqlAdapter::from_config`).

Schema uses `NVARCHAR(MAX)` for the JSON-encoded child id lists and keyword
arrays, `BIT` for the leaf flag, `VARBINARY(MAX)` for raw text chunks (4 MB
default), and `VARBINARY(32)` for both source hashes and summary cache keys.
`MERGE` powers idempotent upserts.

Full-text search is set up best-effort during `migrate`: if the SQL Server
instance has the Full-Text Search component installed, a catalog
`pagebridge_ft` and an index on `pagebridge_nodes` are created. The default
search path in v0.2 falls back to indexed `LIKE` for portability; full
`CONTAINSTABLE`-based ranking lands in v0.3 alongside the rest of the
operational layer.

Operational notes:

- Run with `mssql` Cargo feature: `cargo add pagebridge --features mssql`.
- Microsoft container licensing requires `ACCEPT_EULA=Y`; the integration
  test is gated behind `MSSQL_TEST=1` to keep CI runs predictable.

## Oracle

The Oracle adapter is the first that requires native libraries on the build
host. The underlying `oracle` Rust crate links against Oracle Instant Client
(`libclntsh`). To keep the rest of the workspace buildable on hosts without
Instant Client, the crate has two compile modes:

- **Default (`pagebridge-adapter-oracle`)**: compiles to a stub that returns an
  explicit "Oracle driver not enabled" error from every method. Useful for
  reproducible CI on hosts without Oracle SDK.
- **`oracle-driver` feature**: pulls in the real `oracle` crate. Requires
  Oracle Instant Client present on the build host.

Build the umbrella crate with Oracle support enabled:

```bash
cargo build -p pagebridge --features oracle-driver
```

Schema highlights:

- `pagebridge_nodes` keys `node_id` as `VARCHAR2(512)`, with `CLOB` for
  summaries and keyword JSON arrays, `RAW(32)` for source hashes.
- `pagebridge_raw` stores chunked `BLOB` blocks (1 MB default).
- `pagebridge_summary_cache` uses `RAW(32)` primary key + `BLOB` payload.
- An Oracle Text `CONTEXT` index on `summary` is created best-effort during
  `migrate`; pure-`LIKE` search is the v0.2 fallback when Oracle Text is not
  installed.

Connection pooling is hand-rolled (`parking_lot::Mutex<Vec<Connection>>`) and
every driver call is dispatched onto `tokio::task::spawn_blocking`.

Operational notes:

- Install Oracle Instant Client and ensure `libclntsh` is on the dynamic
  library search path (`LD_LIBRARY_PATH` on Linux, `DYLD_LIBRARY_PATH` on
  macOS).
- Oracle Text option is bundled with Standard Edition and above; if your
  edition lacks it, the adapter still works through the `LIKE` fallback.

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
