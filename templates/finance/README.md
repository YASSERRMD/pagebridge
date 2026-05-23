# pagebridge Finance Template

Tuned for SOX ITGC retrieval workloads: financial documents,
mandatory citations, immutable audit trail.

## Defaults

- **Storage**: Postgres (or any SOX-approved RDBMS).
- **LLM**: Anthropic Claude Sonnet 4.6 (high-quality synthesis).
- **Audit**: mandatory + WORM file sink.
- **Sensitivity**: every document defaults to `Confidential`.
- **Synthesis**: refuses to answer without citations.

## Installation

```
pagebridge template install finance
```

## SOX alignment

See `docs/COMPLIANCE.md` § "SOX (Sarbanes-Oxley) ITGC".
