# Open Retrieval Response Format (ORRF) v1

Status: Draft 0
Editor: Mohamed Yasser (pagebridge)
Reference implementation: `crates/pagebridge-orrf/`

## 1. Motivation

Retrieval-augmented generation systems each invent their own response
format. Downstream agents that combine multiple retrieval systems
must write per-vendor adapters. ORRF defines a single JSON schema
that any retrieval product can emit, so consumers can interoperate
across vendors without ad-hoc translation.

## 2. Wire format

```jsonc
{
  "orrf_version": 1,
  "question": string,
  "answer": string,
  "citations": [
    {
      "id": string,                       // unique within this response
      "content_hash_hex": string,         // sha256(content), 64 hex chars
      "source_uri": string,               // RFC 3986 URI
      "version": integer                  // monotonic per (source_uri)
    }
  ],
  "trace": {
    "steps": integer,
    "total_input_tokens": integer,
    "total_output_tokens": integer,
    "duration_ms": integer
  },
  "receipt": {                            // optional
    "answer_hash_hex": string,            // sha256(answer), 64 hex chars
    "signature_hex": string,              // Ed25519, 128 hex chars
    "key_id": string
  },
  "confidence": float|null                // [0.0, 1.0] if present
}
```

## 3. Conformance

A response is "ORRF v1 conformant" iff:

1. `orrf_version == 1`.
2. Every required field is present and well-typed.
3. Every `citations[i].content_hash_hex` decodes to exactly 32 bytes.
4. If `receipt` is present, `answer_hash_hex` is 32 bytes hex and
   `signature_hex` is 64 bytes hex.

Implementations MUST reject responses that violate any of these.

## 4. Adoption

The reference Rust crate exposes `OrrfResponse::validate()`.
Adapter crates translate native LangChain / LlamaIndex / Haystack /
OpenAI Assistants response shapes to ORRF.

## 5. Versioning

Future versions add fields with sensible defaults; field names and
semantics are immutable. A v2 reader MUST accept a v1 response, and
a v1 reader presented a v2 response SHOULD ignore unknown fields and
accept.

## 6. Submission to IETF

This spec will be submitted as an IETF Internet-Draft titled
"Open Retrieval Response Format" upon pagebridge 2.0 launch.
