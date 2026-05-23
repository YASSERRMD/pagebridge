# Verifiable Retrieval Receipts v1 (draft)

Status: Draft 0
Editor: Mohamed Yasser (pagebridge)

## 1. Motivation

Retrieval-augmented generation systems return answers grounded in a
corpus, but no widely deployed system gives downstream consumers
(auditors, regulators, automated agents) a way to verify, offline and
later in time, that:

1. The answer was produced by a specific LLM with specific sampling
   parameters.
2. A specific set of corpus nodes were used as evidence.
3. The corpus nodes' content has not been altered since the answer was
   produced.

This document defines a vendor-neutral wire format that any retrieval
system can attach to its responses to enable that verification.

## 2. Scope

The receipt covers the answer-level claims listed above. It does NOT
cover:

- Per-token attention or per-sentence groundedness (out of scope for v1).
- Causal counterfactuals (Phase 53 in pagebridge; separate spec).
- Multi-modal content (deferred to v2; only text answers are in scope).

## 3. Receipt structure

Receipts are JSON objects with the following fields, in declaration
order. Encoders MUST emit fields in this order; decoders MUST accept
any order.

```jsonc
{
  "answer_id":        string,         // unique per (workspace, answer)
  "workspace_id":     string,
  "question_hash_hex": string,        // sha256(question_utf8)
  "answer_hash_hex":  string,         // sha256(answer_utf8)
  "corpus_root_hex":  string,         // see §4
  "used_nodes": [
    {
      "node_id":           string,
      "content_hash_hex":  string,    // sha256(node_content_utf8)
      "version":           integer    // monotonic per node
    }
  ],
  "llm": {
    "provider":          string,
    "model":             string,
    "temperature_milli": integer,     // 1000 * temperature
    "top_p_milli":       integer,
    "seed":              integer,
    "revision":          string|null  // vendor-specific
  },
  "prompt_versions":   { string: integer },
  "policy_versions":   { string: integer },
  "trace_hash_hex":    string,        // sha256(canonical_trace)
  "timestamp_ns":      integer,
  "signature_hex":     string,        // Ed25519 over signing_digest, see §5
  "key_id":            string,
  "transparency_log_entry": object|null
}
```

## 4. corpus_root

For each entry in `used_nodes`, compute:

```
leaf = sha256(node_id_bytes || 0x7c || content_hash_bytes || 0x7c || version_be_4)
```

Sort the resulting 32-byte leaves lexicographically. Build a binary
sha256 Merkle tree (Bitcoin convention: odd nodes at any level are
duplicated). `corpus_root_hex` is the hex of the root.

## 5. Signing

`signing_digest = sha256(canonical_bytes_with_signature_blank)` where
`canonical_bytes_with_signature_blank` is the receipt serialized with
`signature_hex` set to the empty string.

The signature is `ed25519_sign(secret_key, signing_digest)`. Encoders
MUST place the resulting hex in `signature_hex`.

`key_id` is an opaque label the verifier uses to look up the matching
public key. We recommend a short hex of the first 8 bytes of
`sha256(public_key_bytes)` for unambiguous identification.

## 6. Verification

A verifier:

1. Parses the receipt JSON.
2. Looks up the verifying key from `key_id`.
3. Recomputes `signing_digest` per §5.
4. Verifies `signature_hex` against `signing_digest` with the verifying
   key. On failure, REJECT.
5. Recomputes `corpus_root_hex` per §4 and compares against the field.
   On mismatch, REJECT.

If both checks pass, ACCEPT and emit the parsed receipt for downstream
processing.

## 7. Transparency log integration

If `transparency_log_entry` is non-null, the verifier MAY additionally
check that the Merkle root of the audit log batch containing this
receipt's emission event appears in the named transparency log at the
specified leaf index. This step is optional in v1 and required only for
systems that advertise log-anchored receipts.

## 8. Conformance

A receipt is "conformant v1" iff:

1. Every required field is present.
2. Hex fields decode to the expected byte length (32 for hashes, 64 for
   the signature).
3. `temperature_milli` and `top_p_milli` are non-negative integers.
4. The signature verifies against the supplied public key.
5. The recomputed `corpus_root_hex` matches.

## 9. Versioning

Future versions add fields with sensible defaults; field names and
semantics are immutable. A v2 receipt is required to be a strict
superset of v1; a v1 verifier presented a v2 receipt MUST ignore
unknown fields, verify the v1 fields it understands, and accept.

## 10. Reference implementation

See `crates/pagebridge-receipt/` in the pagebridge repository. The
`issue_receipt` and `verify_receipt` functions follow this spec.
