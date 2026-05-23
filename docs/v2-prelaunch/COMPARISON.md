# pagebridge 2.0 vs the world

| Capability | pagebridge 2.0 | LangChain | LlamaIndex | Haystack | PageIndex Cloud | ReasonDB |
|------------|----------------|-----------|------------|----------|-----------------|----------|
| Vectorless retrieval | ✓ | ✗ | ✗ | ✗ | ✓ | ✓ |
| Per-retrieval audit log | ✓ chained + signed | ✗ | ✗ | ✗ | ✗ | ✗ |
| Verifiable answer receipts | ✓ (spec + impl) | ✗ | ✗ | ✗ | ✗ | ✗ |
| Bit-deterministic mode | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Time-travel queries | ✓ ask_at(ts) | ✗ | ✗ | ✗ | ✗ | ✗ |
| Adapter coverage (SQL + NoSQL + analytical + KV) | 31+ | 0 (vector-only) | 0 | 0 | proprietary | 1 |
| Hosted SaaS dependency | none | optional | optional | optional | required | optional |
| LLM provider conformance trait | ✓ (determinism + audit baked in) | partial | partial | partial | n/a | partial |
| Sensitivity labels enforced at retrieval | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Per-tenant DRR fair queueing | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Built-in prompt-injection defenses | ✓ + red-team eval | ✗ | ✗ | ✗ | ✗ | ✗ |
| Hot reload of prompts/providers | ✓ arc-swap | ✗ | ✗ | ✗ | ✗ | ✗ |
| Native federated retrieval | ✓ z-score merge | ✗ | ✗ | ✗ | ✗ | ✗ |
| Cost attribution per question | ✓ | ✗ | ✗ | ✗ | n/a | ✗ |
| Causal counterfactual replay | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Shadow traffic A/B router | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Hybrid edge/cloud with confidence escalation | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Browser deployment (WASM) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Open response format (vendor-neutral) | ✓ ORRF-v1 | proprietary | proprietary | proprietary | proprietary | proprietary |
| Vertical templates (gov/health/fin/legal) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| License | MIT + Apache-2.0 | MIT | MIT | Apache-2.0 | proprietary | varies |

The table is not "feature names exist somewhere in marketing"; it is
"the capability ships as a first-class trait or subcommand in the
public release". Sources for each competitor cell live in
`docs/v2-prelaunch/comparison-sources.md`.
