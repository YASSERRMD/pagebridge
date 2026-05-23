# Changelog

All notable changes to pagebridge land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/).

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
