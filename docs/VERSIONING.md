# Versioning policy

Pagebridge follows SemVer for every published crate.

## Public API surface

The following are part of the stable public API and obey SemVer:

- Every type, function, and trait re-exported from the umbrella `pagebridge`
  crate at the crate root.
- The traits `StorageAdapter` and `LlmProvider`, including default method
  bodies and method signatures.
- The shape of `IngestParams`, `UpdateParams`, `Answer`, `AnswerChunk`,
  `Citation`, `QueryTrace`, `TraceStep`.
- The wire format of:
  - the admin HTTP API (`/api/*`),
  - the MCP server tool catalog (`pagebridge.*`),
  - Biscuit token capability strings (`read`, `ask`, `ingest`, `admin`),
  - the Prometheus metric names (`pagebridge_*`).

The following are explicitly **not** stable across minor releases:

- Internal modules (`pagebridge_core::search::*`, `pagebridge_core::ingest::*`).
- Adapter or LLM provider implementations behind their respective traits.
- Prompt template strings in `pagebridge_core::prompts`.
- Trace step contents beyond the documented fields.

## Version bump rules

- **Patch (`1.0.x`)**: bug fixes, doc fixes, dependency bumps without API
  impact. Always safe.
- **Minor (`1.x.0`)**: additive changes. New trait methods come with
  default implementations. New fields on public structs are marked
  `#[serde(default)]` or wrapped in `Option`. New variants on public
  non-exhaustive enums are allowed.
- **Major (`x.0.0`)**: anything that breaks the public API surface above.
  Major bumps come with a `MIGRATION.md` write-up.

## Channels

- `stable`: latest tagged release on crates.io / PyPI.
- `main`: integration branch. Builds green; may contain unreleased work
  staged for the next minor.
- `release/*`: short-lived branches for hotfixes against shipped versions.

## Yanking

We yank a published crate version only for the following reasons:

- A correctness regression that corrupts data.
- A security vulnerability without a patch.

Yanks are announced on the security mailing list and in the project
changelog.

## Deprecation

Public items can be marked `#[deprecated(note = "...", since = "x.y.z")]`
for at least one minor release before removal. Removal requires a major
version bump.
