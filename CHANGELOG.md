# Changelog

All notable changes to pagebridge land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/).

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
