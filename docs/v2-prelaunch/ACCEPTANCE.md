# pagebridge 2.0 Acceptance Checklist

This is the gating document for the 2.0.0 release. Every box must
be checked before crates.io publication.

## 1. Per-phase acceptance

- [x] Phase 38: Tamper-Evident Audit Log — `pagebridge-audit`,
  signing, Merkle anchoring, sinks, AuditHook in facade, CLI
  subcommands, COMPLIANCE doc.
- [x] Phase 39: Verifiable Answer Receipts — `pagebridge-receipt`,
  AnswerReceipt, FacadeReceiptIssuer, verify-receipt CLI, spec.
- [x] Phase 40: Deterministic Retrieval Mode —
  `pagebridge-deterministic`, DeterministicMode, CorpusSnapshot,
  DeterminismContract, CLI flags.
- [x] Phase 41: Time-Travel Queries — `pagebridge-timetravel`,
  SnapshotStore, Overlay, snapshot_at(), CLI flag.
- [x] Phase 42: Major LLM Provider Set — `pagebridge-reranker`,
  `pagebridge-llm-cost`, `pagebridge-llm-routing`, `gemini`,
  `bedrock`, `cohere`, `mlx`, plus convenience constructors for
  Groq/Cerebras/Fireworks/Together/Mistral/HF TGI/Azure/Replicate/
  Modal on the OpenAI provider.
- [x] Phase 43: Major Database Coverage — 22 new adapter crates.
- [x] Phase 44: SLO-Driven Operations — `pagebridge-slo`.
- [x] Phase 45: Continuous Groundedness Monitoring —
  `pagebridge-quality`.
- [x] Phase 46: Reproducible Index Builds — `pagebridge-build`.
- [x] Phase 47: Sensitivity Labels + Access Control —
  `pagebridge-sensitivity`.
- [x] Phase 48: Per-Tenant Resource Isolation — `pagebridge-tenant`.
- [x] Phase 49: Prompt Injection Hardening — `pagebridge-injection`.
- [x] Phase 50: Hot Reload — `pagebridge-hotreload`.
- [x] Phase 51: Federated Retrieval — `pagebridge-federation`.
- [x] Phase 52: Cost Attribution + Budgets — `pagebridge-budget`.
- [x] Phase 53: Causal Trace + Counterfactuals — `pagebridge-causal`.
- [x] Phase 54: Shadow Traffic — `pagebridge-shadow`.
- [x] Phase 55: Hybrid Edge/Cloud — `pagebridge-hybrid`.
- [x] Phase 56: Browser via WASM — `pagebridge-wasm`.
- [x] Phase 57: ORRF v1 — `pagebridge-orrf` + spec.
- [x] Phase 58: Vertical Templates — government / healthcare /
  finance / legal.
- [x] Phase 59: Hardening + Migration docs.
- [x] Phase 60: 2.0.0 version bump + CHANGELOG + LAUNCH +
  COMPARISON docs.

## 2. Cross-cutting acceptance

- [x] Workspace `cargo build --workspace` is clean on the 2.0.0
  candidate cut.
- [ ] Adapter conformance suite: 100 tests per adapter. The
  conformance harness ships in 2.0.0; the per-adapter green-tick
  matrix lives in `docs/v2-prelaunch/CONFORMANCE.md` and is
  populated as drivers are linked.
- [ ] Provider conformance suite: 30 tests per provider. Same
  story.
- [ ] External security audit report attached.
- [ ] Six months of CI fuzzing with zero unfixed findings.
- [ ] Independent third-party reproducibility verification.
- [ ] ORRF v1.0 submitted to IETF.
- [ ] Foundation paperwork filed.
- [ ] At least one named customer per vertical (gov / health / fin
  / legal).
- [ ] Public launch executed.
- [ ] Academic paper submitted.
- [ ] Comparison report published.
- [ ] Migration tools tested against real LangChain and LlamaIndex
  deployments.
- [ ] Build manifests for 2.0.0 byte-identical across three
  independent CI runs.
- [x] `pagebridge --version` returns `2.0.0`.

The boxes marked `[x]` are satisfied by the code that ships in the
2.0.0 candidate. The boxes marked `[ ]` are external dependencies
(audit, foundation, IETF, customers) that the maintainer team
completes before publishing to crates.io.
