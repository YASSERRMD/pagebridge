# Security policy

## Supported versions

Pagebridge follows SemVer. Security fixes are backported as follows:

| Version    | Supported          |
| ---------- | ------------------ |
| 1.x        | :white_check_mark: |
| 0.5.x      | :white_check_mark: until 2027-05-23                |
| < 0.5      | :x:                |

## Reporting a vulnerability

Email **security@pagebridge.io** with:

- A description of the issue (what, where, why it matters).
- Steps to reproduce, ideally with a minimal proof of concept.
- The affected version range.

We aim to acknowledge within 2 business days and ship a fix within 14 days.
For high-severity issues we will issue a coordinated disclosure timeline
with you.

Please do not file public GitHub issues for vulnerabilities. We will credit
reporters in the release notes unless asked otherwise.

## Threat model

Pagebridge is a library and an optional set of HTTP/MCP servers. The threat
model assumes:

- The storage adapter and LLM provider live in the same trust boundary as
  the pagebridge process. Mounting a hostile adapter or provider is out of
  scope.
- Capability tokens (Biscuit) protect the admin web UI and MCP servers when
  enabled. Tokens are short-lived by default and signed by a per-deployment
  root key.
- Plugins (when the loaders ship in v1.1) execute in the host process for
  dylib plugins and in a WASM sandbox for WASM plugins. Dylib plugins are
  fully trusted; WASM plugins see the host interface only.

## Known non-goals

- Cross-process write coordination (pagebridge assumes a single writer per
  database).
- Defense against adversarial LLM outputs that include data exfiltration
  instructions. Mitigate by running navigation-restricted prompts and by
  scoping admin access with capability tokens.
