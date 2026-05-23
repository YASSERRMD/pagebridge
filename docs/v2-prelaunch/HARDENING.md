# pagebridge 2.0 Hardening Notes

Tracking artifact for the pre-launch hardening pass that gates the
2.0 release.

## 1. Fuzz campaign

- Continuous cargo-fuzz coverage on the existing `fuzz/` harnesses
  for: NodeId parser, BM25 tokenizer, prompt template renderer,
  canonical event encoder, ORRF validator.
- Six-month archive of corpora and crashes lives in
  `fuzz/archive/` (gitignored; published to S3 nightly).
- Outstanding: zero unfixed findings as of the v2.0 candidate cut.

## 2. External security audit

- Engagement: a recognised security firm (NCC, Trail of Bits, or
  Cure53) reviews the audit-log signing path, capability-token
  delegation, and the injection-hardening regex set.
- Scope statement: `docs/v2-prelaunch/audit-scope.md` (filed under
  the engagement contract).
- Acceptance: zero critical findings, all medium findings either
  fixed or formally accepted with rationale before 2.0 ships.

## 3. Independent reproducibility

- A community member rebuilds an index from a manifest we publish
  and verifies the produced artifacts byte-match. Tooling:
  `pagebridge build verify --manifest <m.json>` (Phase 46).

## 4. Performance baselines

| Metric | Target | Measured (placeholder) |
| ------ | ------ | ---------------------- |
| p99 retrieval latency (1M nodes, embedded) | < 100 ms | TBD |
| End-to-end ask (M2 Pro, local llama.cpp)  | < 2 s    | TBD |
| Federation across 3 sources (LAN)         | < 300 ms | TBD |
| Audit append throughput                   | > 50k/s  | TBD |

## 5. Documentation polish

- Every `pub` type carries a `///` doc comment.
- Every CLI subcommand has `--help` text that includes an example.
- `docs/spec/` carries: verifiable-receipts-v1, ORRF-v1.
- `docs/COMPLIANCE.md` covers HIPAA, GDPR, SOX, EU AI Act.

## 6. Migration tooling

- `pagebridge migrate from langchain --in <dir>` — read a
  LangChain Document directory, ingest as pagebridge nodes.
- `pagebridge migrate from llamaindex --in <dir>` — same for
  LlamaIndex storage contexts.
- `pagebridge migrate from pageindex --url <url>` — pull from
  PageIndex Cloud.

## 7. Acceptance gate

The 2.0 release ships only when:

1. Sections 1, 2, 3, 4 are all green (no outstanding criticals,
   reproducibility verified, baselines met).
2. Every per-phase checklist in Phases 38 through 58 is closed.
3. The 100-test adapter conformance suite passes for every adapter.
4. The 30-test provider conformance suite passes for every provider.
