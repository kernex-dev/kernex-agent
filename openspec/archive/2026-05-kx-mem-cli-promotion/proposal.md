# Proposal: kx mem CLI promotion

> **Change ID:** `kx-mem-cli-promotion`
> **Status:** LANDED — all 14 mem subcommands shipped across 13 atomic commits between 2026-05-11 and 2026-05-12 on `kernex-dev/kernex-agent@main`. Step 2.11 (`kx mem save`) was the final outstanding subcommand; it landed at `11bdf54` (PR #24 squash) after `kernex-memory 0.8.0` published the typed observation surface.
> **Repo:** `kernex-dev/kernex-agent` (this repo).
> The runtime trait surface this change depends on (`MemoryStore` in
> `kernex-memory`) shipped at v0.6.x via
> [`kernex-dev/kernex` `openspec/archive/2026-05-memory-store-trait-introduction/`](https://github.com/kernex-dev/kernex/tree/main/openspec/archive/2026-05-memory-store-trait-introduction)
> and was extended at v0.8.0 via
> [`kernex-dev/kernex` `openspec/archive/2026-05-typed-observation-table/`](https://github.com/kernex-dev/kernex/tree/main/openspec/archive/2026-05-typed-observation-table).
> `kernex-memory = "0.8.0"` is the current pinned version in `Cargo.toml`.

## Pre-implementation findings

A readiness review on `kernex-agent` confirmed two structural facts the
first draft of this change glossed over. Both are addressed in the
task list.

1. **`kernex-agent` did NOT depend on `kernex-memory` directly.**
   All current memory access goes through `runtime.store.*` (11 distinct
   call sites across `src/main.rs`, `src/commands.rs`,
   `src/scheduler.rs`). The `kernex-memory = "0.6.1"` line in
   `Cargo.toml` was added as a prereq chore in a separate commit; this
   change is the first to import from it.
2. **The `memory-cli` feature was a no-op flag.** `Cargo.toml`
   declares `memory-cli = []` and `#[cfg(feature = "memory-cli")]`
   appears zero times in `src/` today. The feature-graph change that
   introduced the flag deliberately reserved it as a cfg surface for
   this change to fill in.

Other drift surfaced by the review:

3. **No `Command::Mem` variant or `src/mem*` module exists today.**
   Top-level commands today are `Dev`, `Audit`, `Docs`, `Init`,
   `Doctor`, `Pipeline`, `Skills`, `Cron`, `Serve` (the last
   cfg-gated). The `kx mem *` subcommand tree is greenfield.
4. **The existing slash commands** (`/search`, `/facts`,
   `/facts delete <key>`, `/history`, `/memory`, `/cost`, `/clear`)
   are all unconditional and call `runtime.store.*` directly. The
   "refactor to delegate to `mem::cli::*`" step is a real refactor,
   not a rename. The `mem::cli::*` layer does not exist yet.
5. **`kernex-agent` has no bench harness.** Cold-start memory benchmarks
   live only in the runtime workspace. Any cold-start regression gating
   tied to this change must run from there, not from this repo.

## Operator friction

`kx` already ships a project-scoped persistent memory: `kernex-memory`
backed by SQLite + FTS5 with reward-based learning, stored under
`~/.kx/projects/{name}/`. It is reachable from inside the REPL through
five slash commands: `/search`, `/history`, `/memory`, `/facts`,
`/facts delete <key>`. `kx serve` exposes an authenticated HTTP daemon
for job execution but does not expose the memory store.

The infrastructure exists. The agent-native surface does not.

Concrete frictions today:

1. **External shell agents** (Claude Code, Codex CLI, Gemini CLI,
   another `kx` instance in a different project) cannot read or write
   the memory store without spawning an interactive REPL session. There
   is no `kx mem search "auth bug" --json`.
2. **CI runners and shell scripts** cannot consult or seed memory. Any
   workflow that wants to record a decision or fact at the end of a job
   has no entry point.
3. **The REPL prints prose to a TTY**, not JSON. Agents that pipe `kx`
   today must scrape human output. Token cost is high and parsing is
   fragile.
4. **No structured save surface.** Today the slash commands accept
   free-form titles only. There is no way to record the
   What / Why / Where / Learned shape that a coding decision needs to
   stay searchable across sessions.
5. **No soft-delete on facts from the CLI side.**
   `/facts delete <key>` today removes the row entirely; if an agent
   makes a wrong call there is no recovery short of restoring the
   SQLite file from backup. The runtime trait now supports soft-delete
   on facts (`017_soft_delete.sql`); this change wires the CLI to use
   it.

## Solution overview

Promote the existing slash-command surface to a top-level
`kx mem *` subcommand tree, applying the agent-native UX standards
described in the design. The same handlers that power the REPL slash
commands now also power the CLI subcommands. The REPL becomes a thin
caller of the CLI handlers (single source of truth).

The runtime trait surface this change consumes (`MemoryStore`,
`SaveEntry` with structured fields, soft-delete on facts) is already
published in `kernex-memory 0.6.1`. This change is purely the agent-side
surface work.

This change deliberately ships only the subset of subcommands needed to
unblock external agent access plus structured save. It does NOT ship
export / import / sync, workflow analytics, insight commands, HTTP
`/memory/*` endpoints, or any MCP shim. Each of those gets its own
change document once this change lands and the wire format has been
validated by real agent traffic.

## Scope

### In scope

CLI subcommands:

```
kx mem search <query> [--limit] [--since] [--type] [--json] [--compact] [--select]
kx mem get <id>
kx mem history [--last] [--project]
kx mem stats
kx mem facts list
kx mem facts get <key>
kx mem facts add <key> <value>           # --stdin reads value from pipe
kx mem facts delete <key>
kx mem save --type <t> "<title>" \
    [--what <text>] [--why <text>] [--where <path>] [--learned <text>]
kx mem save --stdin                       # accepts SaveEntry JSON
```

Cross-cutting CLI behavior (every subcommand):

- Auto-JSON when `!stdout.is_terminal()`.
- `--json` forces JSON even on a TTY.
- `--compact` projects to high-gravity fields only
  (`id`, `type`, `title`, `updated_at`, `score`).
- `--select fld1,fld2` projects arbitrary fields.
- `--quiet`, `--no-color`, `--dry-run`, `--yes`, `--no-input` available
  where they make sense (defined per command in the spec).
- Exit codes per the taxonomy in `design.md` (0, 2, 3, 4, 5, 7).
- Error format: structured stderr line including the offending flag,
  the corrected usage, and a `Try:` example.

Feature gating:

- The `Mem(MemArgs)` variant on the top-level CLI enum and the entire
  `src/mem/` module are gated behind `#[cfg(feature = "memory-cli")]`.
  The `default = ["agent-claude", "memory-cli", "serve"]` feature set
  keeps this on by default; the minimal-variant build
  (`--no-default-features --features memory-cli`) also ships it. Builds
  without `memory-cli` (`--no-default-features`) compile with `kx mem`
  absent and the binary still functional.
- Slash-command call sites delegate to `mem::cli::*` under the same
  cfg. When `memory-cli` is off, the slash commands print a friendly
  stub message and exit `2`.

REPL changes:

- `/search`, `/history`, `/memory`, `/facts`, `/facts delete <key>`
  delegate to the same `mem::cli::*` async fns as the CLI subcommands.
- Render path differs (REPL prose / table; CLI JSON when piped).
- A parity test harness asserts that REPL handlers and CLI handlers
  return byte-equivalent record sets for the same store state.

### Out of scope (deferred to follow-up changes)

- Export, import, git-chunk sync (separate change).
- HTTP `/memory/*` endpoints in `kx serve` (separate change).
- `kx mcp` MCP shim (separate change).
- Workflow analytics: `stale`, `orphans`, `load`, `reconcile`
  (separate change).
- Insight commands: `health`, `similar`, `patterns`, `conflicts`,
  `decay` (separate change; will land once a capabilities crate
  exists).
- Entry de-duplication via stable keys plus revision counting
  (separate change).
- Vector embeddings or semantic similarity. A future insight surface
  will delegate similarity to a configured provider; no embedding
  store ships in this change.
- Any change to the existing `kx serve` `POST /run`, `GET /jobs`, or
  `/webhook/{event}` routes.

## Why this scope, why now

- **High value, low risk.** Promoting existing handlers to subcommands
  is mechanical; the runtime-side store changes already shipped. This
  change unblocks every external agent immediately.
- **Validates the JSON wire format under real load** before any future
  change builds export / import on top of an unvalidated schema.
- **Keeps follow-up insight surfaces on a clean foundation.**
  Soft-delete, structured save bodies, and the trait surface are
  prerequisites for `conflicts`, `decay`, `similar`, and topic
  upserts. Doing the foundation now means the follow-up work is
  purely additive.
- **Closes the gap that blocks the agent-native CLI positioning.**
  The "kx is a memory layer with an agent attached" value prop is
  unclaimable until the memory layer is reachable from outside the
  REPL.

## Success criteria

The change is shippable when:

1. All `kx mem *` subcommands behave per `spec.md`, with the
   documented exit codes and JSON shapes.
2. Existing REPL slash commands (`/search`, `/history`, `/memory`,
   `/facts`, `/facts delete`) continue to work and now call the same
   handlers as the CLI. No behavior regression.
3. Pre-commit gate passes:
   `cargo build && cargo clippy --all-targets -- -D warnings &&
    cargo test && cargo fmt --check`.
4. Help text for every `kx mem` subcommand is non-empty, lists flags,
   includes at least one example, and ends with a `Try:` line.
5. A throwaway smoke project at `~/.kx/projects/_smoke/` accepts a
   `kx mem save`, returns it via `kx mem search`, and survives a
   `kx mem facts add` / `get` / `delete` round trip via piped JSON.
6. The REPL parity harness asserts the same handler returns the same
   data when invoked via slash command and via CLI subcommand.

## Risks

- **Trait churn.** The `MemoryStore` trait is now fixed at v0.6.x.
  Earlier draft risk of trait churn is gone; additive methods land in
  later workspace minor bumps. The agent code follows.
- **Save struct bikeshed.** What / Why / Where / Learned is a strong
  shape but operators may want flexibility. Mitigation: ship as
  opt-in fields; existing free-text title path remains valid.
- **Soft-delete read filter regressions.** Forgetting the
  `deleted_at IS NULL` clause in any read path silently exposes
  deleted rows. Mitigation: the runtime trait already centralizes the
  filter inside the implementation; the agent code only consumes the
  trait and cannot bypass it. Regression coverage lands in the
  integration tests.
- **JSON shape divergence between REPL and CLI.** If the REPL formats
  prose while the CLI emits structured rows, the same underlying
  handler must support both rendering modes. Mitigation: handlers
  return data; rendering is the caller's concern (REPL renders prose,
  CLI renders JSON or table). The REPL parity harness pins this.
