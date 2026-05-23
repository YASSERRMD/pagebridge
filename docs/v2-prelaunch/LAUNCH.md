# pagebridge 2.0 Launch Plan

## Distribution channels

- crates.io: every workspace member published with the 2.0.0 tag.
- Hacker News: Show HN post.
- LinkedIn: long-form launch post.
- Reddit cross-posts: r/rust, r/MachineLearning, r/LocalLLaMA,
  r/devops, r/datasystems.

## Conference talk submissions

- RustConf 2027 — "Vectorless retrieval at production scale".
- KubeCon — "Running cognitive retrieval as a stateful service".
- Open Source Summit — "Building a foundation-governed Rust project".
- GovTech Summit — "Tamper-evident AI for the public sector".

## Academic paper draft

Working title:

> Pagebridge: Reproducible Cognitive Retrieval over Heterogeneous Backends

Target venues: SIGMOD 2027 industrial track; OSDI 2027 systems track.

## Vertical launch partnerships

- Government: Sharjah Municipality (case study available; Phase 58
  government template was tuned against their workload).
- Healthcare: TBD.
- Legal: TBD.
- Financial services: TBD.

## Conformance program

Any RAG product can claim "ORRF-v1 compliant" by passing the
reference test suite in `crates/pagebridge-orrf/tests/conformance/`.
Pagebridge maintains the certification (free, open). Logo + badge
guidelines in `docs/spec/ORRF-v1-conformance.md` (to be drafted).

## Foundation

Filed paperwork for one of:

- CNCF sandbox project intake.
- Eclipse Foundation working group.
- Linux Foundation neutral home.

The foundation holds the trademark and governs major decisions; the
core maintainers retain the technical roadmap.

## Comparison report

`docs/v2-prelaunch/COMPARISON.md` (to be drafted): pagebridge 2.0
vs. ReasonDB, PageIndex Cloud, LangChain, LlamaIndex, Haystack on:

- Per-retrieval audit (only pagebridge ships it).
- Verifiable receipts (only pagebridge ships it).
- Deterministic mode (only pagebridge ships it).
- Time-travel queries (only pagebridge ships them).
- Adapter coverage (pagebridge has the widest).
- Provider coverage (pagebridge matches LiteLLM at the API level).
- License (MIT/Apache vs others).
