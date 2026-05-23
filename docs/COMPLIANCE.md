# Compliance Mapping

Pagebridge's audit subsystem (`pagebridge-audit`, Phase 38) is designed to
satisfy the per-retrieval audit requirements of several regulated regimes.
This document maps each regulatory clause to the specific pagebridge
artifact that fulfils it.

## HIPAA Security Rule, 45 CFR § 164.312(b) "Audit controls"

> Implement hardware, software, and/or procedural mechanisms that record
> and examine activity in information systems that contain or use
> electronic protected health information.

| Requirement | Pagebridge artifact |
|-------------|---------------------|
| Record activity touching ePHI | Every `Pagebridge::ask`, `ingest_document`, `update_document`, and `remove_document` emits an `AuditEvent` via the `AuditHook` trait. |
| Examine activity | `AuditWriter` chains events per workspace and signs each event with Ed25519. `pagebridge audit verify` re-runs the chain and detects single-byte tampering. |
| Retain audit records | `WormFileSink` enforces append-only WORM semantics locally. Production deployments swap in S3 Object Lock or Azure Immutable Blob. |
| Periodic review | `ElasticSink` and `SplunkHecSink` (gated behind the `http-sinks` feature) ship events to a SIEM. |

## GDPR Article 30 "Records of processing activities"

> Each controller ... shall maintain a record of processing activities
> under its responsibility. That record shall contain ... the categories
> of data subjects and of the personal data; the categories of recipients;
> ... where possible, the envisaged time limits for erasure ...

| Requirement | Pagebridge artifact |
|-------------|---------------------|
| Record categories of personal data | `AuditEvent.sensitivity_label` (Phase 47 will populate this from the document's `SensitivityLabel`). |
| Record purpose | `AuditEvent.policy_decision.applied` carries which policy versions authorised the access. |
| Record recipients | `AuditEvent.principal` records the calling identity (resolved from the Biscuit token, Phase 28). |
| Erasure trail | `Pagebridge::remove_document` emits an `AuditAction::Delete` event chained into the same per-workspace log. |

## SOX (Sarbanes-Oxley) ITGC

| Requirement | Pagebridge artifact |
|-------------|---------------------|
| Access logging | One `AuditEvent` per API boundary. |
| Change control | `AuditAction::Update` and `AuditAction::Delete` are recorded with their before/after state implied by chained ingest events. |
| Segregation of duties | Capability tokens (Phase 28) plus `Capability::Admin` / `Capability::Ingest` separation. |
| Independent audit | The hash chain is verifiable offline by any third party who has the public key and the events file; no pagebridge process is required for verification. |

## EU AI Act, Annex IV (high-risk AI documentation)

| Requirement | Pagebridge artifact |
|-------------|---------------------|
| Logging of automatically generated decisions | Every `ask` produces an event recording the LLM provider, model, and outcome. |
| Traceability | `AuditEvent.parent_event` ties navigation, node-read, BM25, and LLM call sub-events to the root ask event. |
| Robustness evidence | Phase 49 (prompt-injection hardening) ships a red-team eval set and benchmark score. |

## How verification works

```
pagebridge audit tail --dir ./audit --workspace acme --n 100
pagebridge audit verify ./audit/acme.events.ndjson \
    --key ./pagebridge.pub --key-id <key-id>
```

`verify` walks the file, recomputes the canonical sha256 of each event
with `event_hash` and `signature` zeroed, checks that recomputed hash
against the stored `event_hash`, and verifies the Ed25519 signature over
that hash with the supplied public key.

If any event has been altered, the verifier reports `ChainBroken` and
prints the ULID of the offending event. The chain to the *left* of the
break is still verifiable; the chain to the *right* is not (every
following event references the modified `event_hash` and so its
`prev_hash` no longer matches).

## What pagebridge does NOT do for you

- Determine the legal basis for processing personal data. That is the
  controller's responsibility.
- Encrypt data at rest. Use database-level encryption (TDE on Postgres,
  SQLCipher on SQLite, Customer-Managed Keys on MongoDB Atlas) or
  operating-system-level encryption (LUKS, FileVault, BitLocker).
- Generate a Business Associate Agreement (HIPAA), a Data Processing
  Agreement (GDPR), or a SOC 2 report. Those are organisational
  artifacts, not technical features.
- Retain audit records past your storage's retention policy. Configure
  WORM retention at the storage layer.
