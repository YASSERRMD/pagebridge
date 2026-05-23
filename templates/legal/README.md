# pagebridge Legal Template

Tuned for legal-research workloads: paragraph-aware chunking, case
citation extraction, conservative abstention.

## Defaults

- **Storage**: Postgres.
- **LLM**: Anthropic Claude Opus 4.7 (highest synthesis quality).
- **Audit**: enabled + WORM.
- **Sensitivity**: every document `Confidential`.
- **Synthesis**: refuses to answer without citations and abstains
  below 0.55 confidence (this is the typical practitioner's bar).

## Installation

```
pagebridge template install legal
```
