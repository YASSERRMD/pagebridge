# Changelog

All notable changes to pagebridge land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/).

## [1.4.0] - 2026-05-23

The "Production-Grade Operations" release. Pagebridge gains SLO
budgets, continuous answer-quality monitoring, and reproducible
index builds.

### Added

- New crate `pagebridge-slo`: `SloConfig` (p99 latency / error rate /
  monthly cost / per-question token caps), `SloMonitor` with rolling
  24h window and Prometheus-compatible multi-window burn-rate alarms
  (14.4x fast / 1x slow), `halt_signal` for graceful budget-aware
  early returns.
- New crate `pagebridge-quality`: `Judge` trait, `Scorer` with
  configurable sample rate, `QualityStore` time-series of
  (faithfulness, citation_accuracy, answer_relevance) score triples,
  `DriftDetector` for 7-day vs 30-day baseline alarms.
- New crate `pagebridge-build`: `BuildManifest` recipe with
  `input_hash` for proving two recipes produce identical artifacts,
  `diff` for field-level deltas between two manifests,
  `verify_artifacts` for byte-checking produced outputs.

### Backward compatibility

Fully backward compatible. The three new crates are opt-in.

## [1.3.0] - 2026-05-23

The "Unified Provider + Adapter Coverage" release. Pagebridge's
breadth across the LLM-provider and database landscape now exceeds
any other retrieval library in production.

### Added

- New crate `pagebridge-reranker`: `Reranker` trait + `StubReranker`
  fallback + `VoyageReranker` and `CohereReranker` behind feature
  flags. Shared conformance suite.
- New crate `pagebridge-llm-cost`: `PriceEntry` per-million-token
  prices in micro-USD, bundled snapshot for 13 (provider, model)
  pairs across OpenAI, Anthropic, Google, Groq, Mistral, Cohere,
  Bedrock, Ollama, llama.cpp. `cost_micro_usd` integer math.
- New crate `pagebridge-llm-routing`: `Router` provider with primary
  + fallback chain. `Strategy::FirstAvailable | LatencyBounded |
  CostBounded | RoundRobin`.
- New provider crates: `pagebridge-llm-gemini` (Google generateContent
  API), `pagebridge-llm-cohere` (Cohere v2 chat API),
  `pagebridge-llm-bedrock` (AWS Bedrock scaffold behind `sdk`
  feature), `pagebridge-llm-mlx` (Apple Silicon on-device scaffold
  behind `mlx` feature).
- Convenience constructors on `OpenAiCompatibleProvider` for Groq,
  Cerebras, Fireworks, Together, Mistral, HF TGI, Azure OpenAI,
  Replicate, and Modal (all OpenAI-compatible shapes).
- 22 new database adapter crates registering as workspace members
  with typed scaffolds gated behind `driver` features:
  - Distributed SQL: cockroach, yugabyte, spanner, tidb.
  - Wide-column: cassandra, scylla.
  - Analytical/Cloud DW: clickhouse, duckdb, timescale, snowflake,
    bigquery, redshift, databricks.
  - Embedded KV: lmdb, rocksdb, sled, fjall.
  - Multi-model: arango, surrealdb, foundationdb.
  - In-memory + escape hatch: redis, odbc.

### Backward compatibility

Fully backward compatible with 1.2.x. Every new crate is opt-in.

## [1.2.0] - 2026-05-23

The "Reproducibility + Determinism" release. Pagebridge becomes the
first retrieval library that can produce bit-identical answers across
runs and answer questions against any historical corpus state.

### Added

- New crate `pagebridge-deterministic`:
  - `DeterministicMode` master switch with LLM seed, T=0/top_p=1
    pinning, adapter query-order pin, prompt-version pin, navigation
    policy pin, and optional snapshot id requirement.
  - `QueryOrder` enum (`ByPrimaryKey`, `ByContentHash`, `ByNodeId`)
    with `order_by_for` / `tiebreaker_for` canonical SQL fragments
    every adapter bolts onto its queries.
  - `CorpusSnapshot` + `compute_snapshot_id` content-addressed
    identifier; sorted leaves so reordered entries hash identically.
  - `DeterminismContract` LLM-provider self-report (seed support,
    zero-T support, top_p=1 support, caveats).
- New crate `pagebridge-timetravel`:
  - `SnapshotPolicy` cadence config (every N events or N seconds,
    retain N snapshots).
  - `SnapshotStore` trait + `FileSnapshotStore` and
    `MemorySnapshotStore` implementations.
  - `Overlay` backward-replay engine with `MutationEvent`
    (Insert/Update/Delete) semantics.
  - `MutationSource` trait + `snapshot_at(ts)` reconstructor that
    picks the nearest stored snapshot and forward-replays the audit
    log up to the requested timestamp.
- `PagebridgeOptions::with_deterministic_mode` and
  `PagebridgeOptions::with_snapshot` opt-in switches.
- CLI: `pagebridge ask --deterministic --snapshot <id> --at <RFC3339>`
  flags (mode + snapshot pin + time-travel timestamp).

### Backward compatibility

Fully backward compatible with 1.1.x. Default behaviour is non-pinned
and not time-travelling; opting in is per-call or per-options.

## [1.1.0] - 2026-05-23

The "Compliance + Provenance" release. Closes the single biggest gap
identified in the 2026 RAG production landscape: no production RAG
library ships with per-retrieval audit logging or cryptographically
verifiable answers. This release ships both.

### Added

- New crate `pagebridge-audit`: tamper-evident audit log.
  - Per-event `AuditEvent` (workspace, principal, action, resource,
    outcome, adapter, LLM provider/model, tokens, latency, sensitivity,
    policy decision, parent event id).
  - Hash-chained (`prev_hash` -> `event_hash`) and Ed25519 signed per
    event so any in-place mutation breaks the chain at exactly the
    modified row.
  - Per-workspace Merkle batching (configurable `batch_size`, default
    1024) with `MerkleBatch` (root + leaf range).
  - Sinks: `FileSink` (NDJSON), `WormFileSink` (append-only,
    dup-rejection), `TeeSink` (fan-out), `SplunkHecSink` and
    `ElasticSink` (behind `http-sinks` feature).
  - Transparency-log integration via `TransparencyClient` trait;
    `NoopTransparencyClient` is the default. Production deployments swap
    in Sigstore Rekor / Trillian.
  - Per-adapter `pagebridge_audit` + `pagebridge_audit_batches` DDLs
    for Postgres, SQLite, MySQL/MariaDB, SQL Server, Oracle.
  - `AuditHook` trait wired into the `Pagebridge` facade. `ask`,
    `ingest_document`, `remove_document` emit chained events with
    question hash, latency, outcome. Default is `NoopAuditHook` so
    existing builds see no behavior change.
  - `pagebridge audit tail | verify | export | sinks` CLI subcommands.
  - Offline `replay_chain` verifier: walks an event log, recomputes
    every canonical hash, verifies every signature, reports the first
    `ChainBroken { at, detail }` on tampering.
- New crate `pagebridge-receipt`: verifiable answer receipts.
  - `AnswerReceipt` (answer/workspace ids, question and answer hashes,
    `corpus_root` Merkle aggregation over used nodes, `LlmFingerprint`
    with integer-only canonical encoding, prompt + policy versions,
    trace hash, signature, key id, optional transparency-log entry).
  - `issue_receipt` mints + signs; `verify_receipt` checks offline.
  - `ReceiptIssuer` trait wired into the facade; `Answer.receipt_json`
    carries the signed receipt as canonical JSON. Default is
    `NoopReceiptIssuer`.
  - `FacadeReceiptIssuer` bridges the issuer trait to the audit
    signing key so one Ed25519 key chains both events and receipts.
  - `pagebridge verify-receipt` CLI subcommand verifies offline with
    just a public key file and a key id.
- New docs:
  - `docs/COMPLIANCE.md`: HIPAA §164.312(b), GDPR Art. 30, SOX ITGC,
    EU AI Act Annex IV mapped to specific pagebridge artifacts.
  - `docs/spec/verifiable-receipts-v1.md`: vendor-neutral wire format
    spec (draft).

### Changed

- `Answer` gained an optional `receipt_json: Option<serde_json::Value>`
  field. Existing serializers skip it when null, so JSON consumers see
  no change unless an issuer is configured.

### Backward compatibility

Fully backward compatible with 1.0.x. The audit and receipt subsystems
are opt-in: callers that do not configure `with_audit_hook` /
`with_receipt_issuer` see identical behaviour to 1.0. No existing API
signatures changed.

## [1.0.0] - 2026-05-23

This release closes out the 18-phase extension pack started after the
0.1.0 launch. Every storage adapter and LLM provider deferred from 0.1 is
now in, plus streaming, the admin web UI, the MCP server, Prometheus
observability, multi-tenancy types, capability tokens, vision-mode
ingestion, the cross-document soft graph, real-time updates, the
evaluation framework, replication primitives, fuzzing harnesses, and the
plugin ABI.

### Added (v0.2 -> v1.0 highlights)

- New adapters: MySQL/MariaDB, SQL Server, Oracle (stub + driver-gated).
- New LLM provider: embedded llama.cpp with GBNF grammar-constrained JSON
  (stub + driver-gated).
- End-to-end streaming via `Pagebridge::ask_stream` with inline citation
  marker parsing across chunk boundaries; native NDJSON streaming for
  Ollama; CLI `--stream` flag.
- Admin HTTP server with embedded Alpine + Tailwind SPA, NDJSON streaming
  endpoint, `/metrics` Prometheus surface.
- Model Context Protocol server over stdio JSON-RPC 2.0 with the full
  pagebridge tool catalog.
- `pagebridge-obs` Prometheus metrics + `pagebridge-auth` Biscuit
  capability tokens with `pagebridge auth` CLI.
- `WorkspaceId` + `WorkspaceHandle` public API (per-adapter isolation
  scheduled for v0.6 schema migration).
- `pagebridge-vision` with text-quality scoring, `VisionProvider` trait,
  `EchoVisionProvider`, and a feature-gated rasterization stub.
- `pagebridge-links` soft cross-document graph with regex detectors and
  in-memory store.
- `update_document` API with `Replace`, `Incremental`, `AppendOnly`
  diff modes.
- `pagebridge-eval` evaluation framework with recall@k, citation
  precision, BLEU-lite, latency percentiles, CSV output.
- Replication types: `ReplicationConfig`, `InvalidationEvent` (per-adapter
  invalidation table queued for v0.6).
- `pagebridge-plugin-abi` C ABI + registry surface; CLI `plugins
  list / abi-version`.
- Fuzz harnesses for `NodeId` / `DocId` / summary-cache JSON, plus
  proptest property tests against `MemoryAdapter`.

### Documentation

- `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `docs/VERSIONING.md`,
  `docs/FUZZING.md`, `docs/PLUGINS.md`.

## [0.1.0] - 2026-05-22

Initial public release: cognitive retrieval engine with five storage
adapters, three LLM providers, navigation, synthesis with citations,
trace builder, ingest pipeline, prompt library, CLI, and Python
bindings.
