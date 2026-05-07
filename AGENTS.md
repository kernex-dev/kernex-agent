# AGENTS.md

This repository follows the three-layer Claude documentation model. Project
instructions live in [`CLAUDE.md`](CLAUDE.md) (Layer 2) and on-demand context
lives in [`.claude/docs/`](.claude/docs/) (Layer 3).

If your agent (Codex, Cursor, Qwen-Coder, Gemini, Aider, etc.) reads
`AGENTS.md` rather than `CLAUDE.md`, treat the two files as equivalent: the
canonical guidance is in `CLAUDE.md`. There is no separate AGENTS-only
contract.

## Quick links

- Project rules and dev commands: [`CLAUDE.md`](CLAUDE.md)
- Static reference (source layout, serve API, provider matrix): [`.claude/docs/CONTEXT.md`](.claude/docs/CONTEXT.md)
- Append-only learnings (decisions, gotchas, audit punch-lists): [`.claude/docs/LEARNINGS.md`](.claude/docs/LEARNINGS.md)
- Security policy and threat-model caveats: [`SECURITY.md`](SECURITY.md)

## Pre-commit gate

Every change must pass:

```
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --check
```

The bin uses `#![deny(warnings)]` and `#![deny(clippy::unwrap_used,
clippy::expect_used)]`, with a paired `#![cfg_attr(test, allow(...))]` for
test code only. Library / production code stays under the deny.

## Authorship

Do not add `Co-Authored-By` trailers or rewrite AI-tool authorship via
`.mailmap`. Commits are signed by the human who reviewed and pushed them; the
agent's involvement is a working detail, not a contributor.
