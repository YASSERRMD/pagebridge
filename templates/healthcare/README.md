# pagebridge Healthcare Template

Tuned for HIPAA-aligned retrieval over Protected Health Information.

## Defaults

- **Storage**: Postgres (TDE recommended at the DB layer).
- **LLM**: AWS Bedrock (Anthropic via Bedrock has a BAA option;
  OpenAI is intentionally NOT a default).
- **Audit**: mandatory; events flow to file + WORM + Splunk.
- **Sensitivity**: every document defaults to `Phi`; only roles
  enumerated in `allow_phi_for_roles` may retrieve.
- **Budget**: a default monthly cap with alert and hard-stop.

## Installation

```
pagebridge template install healthcare
```

## Compliance posture

See `docs/COMPLIANCE.md`. This template ships the
configuration knobs that a HIPAA risk assessment will look for. You
remain responsible for:

- Signing a BAA with each cloud provider you connect.
- Reviewing access logs (the audit chain is verifiable offline).
- Implementing access controls at the application layer above
  pagebridge.
