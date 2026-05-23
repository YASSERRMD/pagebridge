# Contributing to pagebridge

Thanks for considering a contribution. Pagebridge is built around small,
reviewable PRs that ship one capability each. The guidelines below mirror
what the maintainers actually use day-to-day.

## Quick start

```bash
git clone https://github.com/YASSERRMD/pagebridge
cd pagebridge
cargo build --workspace --all-targets
cargo test --workspace
```

A first-PR checklist is in `docs/CONTRIBUTING_CHECKLIST.md`.

## House style

- **Atomic commits.** One logical change per commit. Conventional Commits
  format (`feat(scope): ...`, `fix(scope): ...`, `chore(scope): ...`,
  `docs(scope): ...`, `test(scope): ...`). The commit messages and PR
  descriptions visible in the history are the model.
- **No em-dashes** anywhere in prose, code, or comments. Use a regular
  hyphen or rephrase. (We grep for U+2014 in CI.)
- **`clippy::pedantic` clean.** Run `cargo clippy --workspace --all-targets`
  before pushing. Allow individual lints with `#[allow(...)]` and a short
  reason in the surrounding doc comment.
- **No `unwrap()` or `panic!`** in non-test library code. Use
  `thiserror`-derived errors and bubble up.
- **Async-first public API.** Sync helpers, where they exist, are private
  implementation details.
- **License header**: not required. The workspace `LICENSE` covers every
  crate.

## Workflow

1. Fork and branch (`feat/<topic>` or `fix/<topic>`).
2. Add tests next to the change.
3. Run `cargo build --workspace --all-targets` and
   `cargo test --workspace` locally.
4. Push and open a PR.
5. A maintainer reviews. Squash if you have noisy fixup commits; otherwise
   the merge keeps your atomic history.

## Where to start

- Issues labelled `good-first-issue` are scoped for newcomers.
- The "Plugin author guide" (`docs/PLUGINS.md`) is the entry point if you
  want to build an integration without modifying core.
- Adapter and provider conformance tests live in
  `crates/pagebridge-core/tests/` and apply equally to every backend.

## Reviewing

Maintainers look for: passing tests, no clippy regressions, documentation
for new public API, and PR descriptions that match the actual diff. We do
not block on bikeshed style points.

## Contact

- Issues: <https://github.com/YASSERRMD/pagebridge/issues>
- Security: see `SECURITY.md` for the responsible disclosure process.
