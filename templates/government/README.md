# pagebridge Government Template

This template configures pagebridge for public-sector policy and
regulation retrieval. Defaults:

- **Storage**: embedded redb + tantivy (no external dependencies).
- **LLM**: local llama.cpp with a Qwen 2.5 model (no cloud egress).
- **Audit**: tamper-evident chain + WORM file sink for retention.
- **Determinism**: enabled by default; queries are reproducible.
- **Sensitivity**: every ingested document defaults to "Internal";
  auto-classifier flags PII categories.
- **Tokenizer**: snowball + Arabic stemmer for MENA deployments.

## Installation

```
pagebridge template install government
```

## What you get

A configured pagebridge appliance ready to ingest:

- National regulations (Federal Decree-Law, Cabinet Resolutions).
- Municipal circulars.
- Agency-level policy documents.

## What you do NOT get

- A cloud LLM connection. Use `pagebridge llm add <provider>` if you
  need one for synthesis quality on hard queries.
- Federation across other agencies. Configure
  `[federation.sources]` per the docs.

## Compliance

The default WORM audit sink, deterministic mode, and least-privilege
sensitivity policy align with the typical procurement requirements
for public-sector AI systems. See `docs/COMPLIANCE.md` for details.
