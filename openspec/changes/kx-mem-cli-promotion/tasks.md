# Tasks: kx mem CLI promotion

> **Reference:** [proposal.md](proposal.md), [spec.md](spec.md),
> [design.md](design.md).
> Each task is sized to under 2 focused hours.

## Coordination rules

1. The runtime trait surface (`MemoryStore`, `SaveEntry`, soft-delete
   on facts) is shipped in `kernex-memory 0.6.1` on crates.io. The
   typed-row surface (`MessageRow`, `HistoryRow`, `get_message_by_id`,
   server-side `since: Option<SystemTime>`) that Step 2.4 depends on
   lands in `kernex-memory 0.7.0` (`memory-typed-row-shape` Slice B).
   The direct dep is pinned in `Cargo.toml`. Step 2.4 ships in a
   paired migration commit that bumps `kernex-* 0.6.2 -> 0.7.0` at
   the same time the handler wires through; every other handler in
   this change is pure agent-side work.
2. Pre-commit gate must pass before any commit:
   `cargo build && cargo clippy --all-targets -- -D warnings &&
    cargo test && cargo fmt --check`.
3. No `Co-Authored-By` trailers, no `--no-verify`, no auto-commit.

---

## Step 1 — REPL parity harness (lands first)

Per ADR-009, the parity harness is the enforced invariant that keeps
REPL and CLI in sync. Land it before any handler work so each
subsequent handler commit can run the harness and fail loudly on
divergence.

### 1.1 Author `tests/mem_repl_parity.rs` skeleton

- New file. Uses `tempfile::TempDir` to scaffold an isolated
  `~/.kx/projects/_parity_<rand>/` per test.
- Seed helper: insert a canonical fixture (5 observations, 3 facts,
  one soft-deleted of each) so every parity test starts from the
  same state.
- Walk-the-matrix helper: takes a slash-command string and a CLI
  argv, runs both against the seeded store, returns the record set
  (post-handler, pre-render) for byte-equivalence assertion.
- Skeleton tests for each row in the ADR-009 parity matrix; all
  marked `#[ignore]` until the corresponding handler exists.

### 1.2 Wire harness into CI

- `cargo test --features memory-cli` runs the parity tests as part of
  the standard suite.
- Document in `tests/README.md` (or top of `mem_repl_parity.rs`) how
  to run a single parity test in isolation.

---

## Step 2 — kx CLI surface

### 2.1 Add `Mem(MemArgs)` to `cli.rs`

> **Feature gate:** the `Mem(MemArgs)` variant and its `MemArgs` enum
> are gated behind `#[cfg(feature = "memory-cli")]`. The
> `default = ["agent-claude", "memory-cli", "serve"]` feature set keeps
> this on by default; the minimal variant
> (`--no-default-features --features memory-cli`) also ships it. Builds
> without `memory-cli` (`--no-default-features`) compile with
> `kx mem` absent and the binary still functional.

- Edit `src/cli.rs`. Add a top-level `Mem` subcommand whose `MemArgs`
  is a clap subcommand enum: `Search`, `Get`, `History`, `Stats`,
  `Facts(FactsArgs)`, `Save(SaveArgs)`.
- The `Mem` variant on the top-level enum carries
  `#[cfg(feature = "memory-cli")]`. The `MemArgs` enum lives in
  `src/mem/cli.rs` (also gated; see 2.2) so it does not compile when
  the feature is off.
- All cross-cutting flags (`--json`, `--compact`, `--select`,
  `--quiet`, `--no-color`, `--no-input`) are global to the `Mem`
  subcommand and inherited by children where they apply.

### 2.2 Scaffold `src/mem/` module

> **Feature gate:** the entire `src/mem/` module is gated behind
> `#[cfg(feature = "memory-cli")]`. Add `#[cfg(feature = "memory-cli")]
> pub mod mem;` (or `mod mem;`) at the call site in `src/main.rs` (or
> `src/lib.rs` if extracted), and a top-of-file
> `#![cfg(feature = "memory-cli")]` inside `src/mem/mod.rs`. Verify
> `cargo build --no-default-features` compiles with the module absent.

- New directory. Files (all under the module-level
  `#[cfg(feature = "memory-cli")]`):
  - `mod.rs` (dispatcher; pub re-exports).
  - `cli.rs` (one async fn per subcommand; returns the typed record).
  - `render.rs` (auto-JSON detection, `--compact`, `--select`,
    table renderer for TTY).
  - `types.rs` (CLI-side wrappers if needed; otherwise re-exports
    from `kernex_memory`).
  - `errors.rs` (CLI error type that maps to `MemoryError` and to
    exit codes per ADR-005; owns the `Try:` hint copy).

### 2.3 Implement `kx mem search`

- Wire to `MemoryStore::search_messages`.
- Apply `--limit` (default 10), `--since` (parse `30d`, `12h`,
  `90m`, `5w`), `--type` (validate against `ObservationType`).
- Auto-JSON, `--compact`, `--select` per ADR-006.
- Cover spec scenarios `S-search-1` through `S-search-6`.
- Unblocks parity tests for `/search`.

### 2.4 Implement `kx mem get`

- Wire to `MemoryStore::get_message_by_id` (added in
  `kernex-memory 0.7.0`). Exit `3` on `None`. Soft-deleted is
  invisible (CC-9, S-get-3). Argument is `String` (UUID), not `i64`.
- Ships in the same paired-migration commit that bumps
  `kernex-* 0.6.2 -> 0.7.0`; pre-merge gate must pass against the
  newly published 0.7.0 crates on crates.io.

### 2.5 Implement `kx mem history`

- Wire to the history surface. Default `--last 20`.
- `--project <name>` overrides cwd-based detection. Unknown project
  is exit `3` (S-history-4).
- Unblocks parity tests for `/history`.

### 2.6 Implement `kx mem stats`

- Wire to `MemoryStore::get_memory_stats`. Empty project is valid
  (S-stats-2).
- Unblocks parity tests for `/memory` (the legacy slash name routes
  here per S-repl-3).

### 2.7 Implement `kx mem facts list`

- Wire to `MemoryStore::list_facts`. Empty list returns `[]` (CC-5).
- Unblocks parity tests for `/facts`.

### 2.8 Implement `kx mem facts get`

- Wire to `MemoryStore::get_fact`. Missing key is exit `3` with hint.

### 2.9 Implement `kx mem facts add`

- Inline value; `--stdin` mutually exclusive with positional value.
- Empty value is exit `2` with hint to use `facts delete`.
- Existing key is upsert (S-facts-add-3).

### 2.10 Implement `kx mem facts delete`

- Soft-delete via `MemoryStore::soft_delete_fact`. Idempotent absence
  (S-facts-delete-3).
- Unblocks parity tests for `/facts delete <key>`.

### 2.11 Implement `kx mem save`

- Inline mode: `--type`, title positional, optional `--what`,
  `--why`, `--where`, `--learned`.
- `--stdin` mode: parse SaveEntry JSON from stdin; mutually exclusive
  with inline fields (S-save-6).
- Validate all required fields before any DB call (S-save-3, S-save-4,
  S-save-5, S-save-7).
- Sandbox refusal maps to exit `4` (S-save-8).

### 2.12 Auto-JSON renderer

- `src/mem/render.rs`: detect TTY via `std::io::IsTerminal`.
- TTY: human table or short prose per command.
- Non-TTY or `--json`: `serde_json::to_string`.
- `--compact` projects to high-gravity fields; `--select` to a custom
  set; unknown fields exit `2`.

### 2.13 Structured CLI errors

- `src/mem/errors.rs`: enum with one variant per documented exit
  code. Each variant carries a `message` and a `hint` (the `Try:`
  line). On `--json` or non-TTY stderr, emit one-line JSON object.

### 2.14 Refactor REPL slash commands to delegate to `mem::cli`

> **Feature gate:** the slash-command branches that delegate to
> `mem::cli::*` are gated behind `#[cfg(feature = "memory-cli")]`.
> When `memory-cli` is off, `/search`, `/history`, `/memory`,
> `/facts`, and `/facts delete <key>` print a friendly "this build
> was compiled without `memory-cli`; reinstall with `cargo install
> kernex-agent` to enable" stub, exit code 2. The stub branch is
> gated `#[cfg(not(feature = "memory-cli"))]` so the call sites stay
> tidy.

- Edit `src/commands.rs`. `/search`, `/history`, `/memory`, `/facts`,
  `/facts delete <key>` all call the same `mem::cli::*` async fns
  (under the `memory-cli` cfg).
- REPL renders for human via the existing TTY renderer (or a
  per-command formatter).
- Existing REPL tests still pass; add coverage for the
  `--no-default-features` build path that confirms the stub message.
- Flip the `#[ignore]` attribute off on each `mem_repl_parity.rs`
  test as the corresponding handler lands.

### 2.15 Help text contract

- For every `kx mem *` subcommand: synopsis, flags, at least one
  example, exit codes section, `Try:` line.
- Lint test: `kx mem search --help` includes the literal substring
  `Try:`. Same for every subcommand.

### 2.16 Integration tests `tests/mem_cli.rs`

- One async test per spec scenario (`S-search-*`, `S-get-*`, ...).
- Use `tempfile::TempDir` for isolated `~/.kx/projects/_test_<rand>/`.
- Cover both renderers: TTY (assert table substrings) and non-TTY
  (assert JSON shape via `serde_json::Value`).

### 2.17 Golden JSON shape tests

- Pin one canonical fixture per command under `tests/fixtures/mem/`.
- Loading the fixture must round-trip via `serde_json` without loss.

### 2.18 Smoke script

- `scripts/smoke-mem.sh`: scaffolds `~/.kx/projects/_smoke_<pid>/`,
  runs `save -> search -> get -> facts add/get/delete -> stats`, asserts
  exit codes and JSON shape, cleans up. Used in manual verification.

### 2.19 Document the new surface in `CLAUDE.md` and `README`

- Add a `kx mem` section to `CLAUDE.md` under "Architecture"
  pointing at this design doc.
- Add usage examples to `README.md`.
- Note the REPL slash commands now delegate (single source of truth).

### 2.20 Cut `kernex-agent` release

- Bump `kernex-agent` version per the matrix
  (e.g., `0.5.0` -> `0.6.0`).
- `CHANGELOG.md` entry under `[Unreleased]` referencing this change.
- Tag and publish per the agent repo's release flow.

---

## Step 3 — Verify and archive

### 3.1 Run the full pre-commit gate

- `cargo build` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test` green (new + existing).
- `cargo test --no-default-features` green (stub-path coverage).
- `cargo fmt --check` clean.

### 3.2 Smoke against a real `~/.kx/projects/<name>/`

- Pick a non-throwaway local project that already has memory.
- Run every `kx mem *` subcommand against it.
- Confirm REPL slash commands still produce the same data.
- File any drift as a follow-up issue, not a hotfix in this change.

### 3.3 Confirm REPL parity tests all pass

- Every `#[ignore]` from Step 1 is now removed.
- `cargo test --test mem_repl_parity` is green.

### 3.4 Archive this change

- After the PR lands:
  `mv openspec/changes/kx-mem-cli-promotion/
     openspec/archive/2026-MM-kx-mem-cli-promotion/`.
- Add a one-line header to each file noting the merge date and
  commit SHA.

---

## What is intentionally absent from this task list

- Export / import / sync — separate change.
- `kx serve /memory/*` HTTP endpoints — separate change.
- `kx mcp` MCP shim — separate change.
- Workflow analytics (`stale`, `orphans`, `load`, `reconcile`) —
  separate change.
- Insight commands (`health`, `similar`, `patterns`, `conflicts`,
  `decay`) — separate change.
- Entry de-duplication via stable keys plus revision counting —
  separate change.
- Performance optimization beyond the partial index already in the
  shipped migration.
- Telemetry, metrics, OTel integration.

These all wait for this change to land and the JSON wire format to
be validated by real agent traffic.
