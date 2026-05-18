# Proposal: Claude Code adapter + 7-stage install pipeline

> **Status:** ACTIVE · **Date:** 2026-05-18
> **Pairs with:** [design.md](design.md), [spec.md](spec.md), [tasks.md](tasks.md)
> **Target:** `kernex-dev/kernex-agent`
> **Upstream:** `kernex-adapter-core 0.8.0+` (trait surface consumed as-is, no upstream change required)

## Why this change exists

This SDD lands the first concrete `Adapter` implementation in `kernex-agent` (Claude Code) plus the 7-stage configurator pipeline. The trait surface in `kernex-adapter-core 0.8.0` is the contract; everything in this SDD wires kernex-agent against that contract.

It is the first cut at the single-binary install story: `kx install --agent claude-code --preset solo-dev` against a clean `$HOME` produces a valid `CLAUDE.md`, `mcp.json`, and `output-style.md`, with a full per-stage audit trail at `~/.kx/audit/install-<ISO8601>.jsonl` and automatic rollback on failure.

The SDD also exercises the per-adapter binary-size discipline for the first time: this adapter is measured against an 800 KiB delta budget against the default-build baseline.

## What this SDD ships

- **`src/configurator/`** module hosting the 7-stage pipeline (DETECT, RESOLVE, REVIEW, BACKUP, APPLY, VERIFY, REPORT) as a sequence of stage-typed functions. Not a generic framework.
- **`src/adapters/claude.rs`** behind `#[cfg(feature = "agent-claude")]` implementing `kernex_adapter_core::Adapter` for `AdapterId::ClaudeCode`.
- **`templates/claude/`** with three compiled-in templates: `CLAUDE.md.tmpl`, `mcp.json.tmpl`, `output-style.md.tmpl`. Loaded via `include_str!`.
- **`src/install/cli.rs`** wiring the `kx install` clap subcommand (`--agent claude-code --preset solo-dev` and friends) into the configurator entry point.
- **`src/install/audit.rs`** writing the JSONL append-only audit log at `~/.kx/audit/install-<ISO8601>.jsonl`.
- **`src/install/preset.rs`** defining kernex-agent's own internal `Preset` type plus a `resolve_preset(name)` function with an inline `solo-dev` body. No external preset crate dep.
- **Integration tests** under `tests/install_claude_*.rs` driving `kx install --agent claude-code --preset solo-dev` against a `tempfile::TempDir` fixture HOME, asserting all exit-signal clauses.
- **Per-adapter size measurement** added to the binary-size CI workflow: the existing default-build size comment gets a per-adapter delta line for `agent-claude`.

## Out of scope

- **Other adapters.** Additional adapter implementations ship in a follow-up change. The configurator code is shaped so adding an adapter is "register one more `Arc<dyn Adapter>` in the `AdapterRegistry` plus a templates directory," but this change only registers and exercises ClaudeCode.
- **TUI.** `--yes` and `--dry-run` are the only confirmation flags.
- **Preset catalog.** `solo-dev` ships as an inline `Preset` value. Other preset names return `InstallError::UnknownPreset`. A real preset catalog is a separate future change.
- **Backup retention policy.** This change writes backups to `~/.kx/backups/<ISO8601>-<agent>.tar.gz`. Pruning, rotation, or "keep last N" rules are deferred.
- **Backend pluggability for the audit log.** JSONL on local disk only. Remote audit endpoints are out of scope.
- **MCP server runtime.** This change generates a valid `mcp.json` registration but does not start, manage, or proxy MCP servers. Server lifecycle is the user's responsibility.

## Ratified design locks

These are settled before the implementation PR opens. Each is restated as an ADR in [design.md](design.md) and tested as a spec scenario in [spec.md](spec.md).

- **E-LOCK-01 — Pipeline shape.** Seven stages, strict order, no skipping. Each stage takes the prior stage's typed output and returns its own. Failure short-circuits to rollback.
- **E-LOCK-02 — Templates are compiled-in.** `include_str!` for `CLAUDE.md.tmpl`, `mcp.json.tmpl`, `output-style.md.tmpl`. No runtime template directory lookup.
- **E-LOCK-03 — Audit log is JSONL on local disk, append-only.** One line per event. Rollback writes its own events; it does not edit or delete prior events.
- **E-LOCK-04 — BACKUP is a tarball.** `~/.kx/backups/<ISO8601>-<agent>.tar.gz` containing every file the APPLY stage will touch. Restore is `tar -xzf` over the original paths.
- **E-LOCK-05 — Rollback is automatic on APPLY failure.** Per-step receipts are walked backwards; each step's undo is invoked. No prompt, no manual recovery in the happy unwind path.
- **E-LOCK-06 — Sandbox refusal for writes outside configured roots.** Mirrors the memory CLI's ADR-005: the adapter probes its target path before writing; if the sandbox layer refuses, the stage fails clean and rollback fires.
- **E-LOCK-07 — `solo-dev` preset ships inline.** `src/install/preset.rs` defines an internal `Preset` struct (`adapters: Vec<AdapterId>`, `components: Vec<String>`) plus `resolve_preset(name) -> Result<Preset, InstallError>`. For this change the only wired name is `solo-dev`; unknown names return `InstallError::UnknownPreset`.
- **E-LOCK-08 — Size budget is normative.** `agent-claude` contributes ≤ 800 KiB to the default-build. If breached, the adapter moves to opt-in (`default` features change) and the SDD revises before merge.
- **E-LOCK-09 — Stage tracing is structured.** Every stage emits one JSON object per stage with `{stage, status, started_at, ended_at, receipts, errors}`. The trace is part of the install audit entry, not a separate log.
- **E-LOCK-10 — Per-stage exit codes follow the memory CLI's ADR-005.** Hard failures map to existing `CliError` variants; transient failures (sandbox busy, filesystem locked) surface as exit 7 per the `CliError::Transient` classifier shipped in `kx mem search` (FU-D-AG-05).

## Cross-cutting invariants

Reused across every stage and tested in spec.md as `E-CC-*`:

- **E-CC-1.** All file writes pass through the audit log writer. There is no direct `fs::write` call outside `src/install/audit.rs` and the per-stage handlers it dispatches.
- **E-CC-2.** All file writes are sandbox-probed before the write. If the sandbox refuses, the stage fails clean (exit 7, transient).
- **E-CC-3.** Every stage emits exactly one trace event on success and one on failure. No silent stages.
- **E-CC-4.** Failures roll back automatically; the audit log records both the failure cause and the rollback events.
- **E-CC-5.** The default-build size delta for `agent-claude` is measured on every PR and reported in the binary-size comment. This change does not merge if the delta exceeds 800 KiB.
- **E-CC-6.** No write outside `$HOME` and `~/.kx/`. Adapter targets (`$HOME/.claude/`, `$HOME/.config/<agent>/`, etc.) are the only sanctioned write roots.
- **E-CC-7.** No network calls during install. DETECT does not phone home; APPLY does not fetch templates. Air-gapped install must succeed.

## Sequencing

1. **Pre-execution audit** (§0 in tasks.md). Verify `kernex-adapter-core` and `kernex-sandbox` at the workspace pin (≥ 0.8.0) are live on crates.io; verify `src/adapters/claude.rs` is the one-line reserved file; verify no install or configurator directories exist. Audit completed 2026-05-18 against `kernex-dev/kernex-agent@49e2218`: all checks green, baseline default-build = 11,113,824 bytes (10.60 MiB) on macOS aarch64.
2. **Configurator scaffolding** (§1). `src/configurator/mod.rs` plus seven empty stage modules with typed inputs/outputs and a stub `run()` returning `unimplemented!`. Compiles, no behavior.
3. **Audit writer** (§2). `src/install/audit.rs` + JSONL schema + 5 unit tests covering append-only, rollback-record, structured-trace semantics.
4. **Preset shim** (§3). `src/install/preset.rs` with the inline solo-dev shortcut + 3 unit tests.
5. **Claude adapter implementation** (§4). `src/adapters/claude.rs` implements `Adapter` trait + templates loaded via `include_str!` + 6 unit tests covering detect/install_command on a tempfile HOME.
6. **Stage implementations in order** (§5–§11). DETECT first (smallest blast radius), then RESOLVE, REVIEW, BACKUP, APPLY, VERIFY, REPORT. Each stage lands as one commit with its unit tests.
7. **CLI wiring** (§12). `kx install` clap subcommand + integration tests driving the full happy path and the rollback path.
8. **Size measurement** (§13). Update binary-size CI workflow to report `agent-claude` delta. Verify default-build delta is ≤ 800 KiB.

## What "done" looks like

This SDD is DONE when:

1. `kx install --agent claude-code --preset solo-dev` against a clean `$HOME` (a `TempDir` in tests) writes a valid `CLAUDE.md`, `mcp.json`, and `output-style.md` (the latter conditional on the preset).
2. A structured audit entry exists at `~/.kx/audit/install-<ISO8601>.jsonl` containing one event per stage plus a top-level summary.
3. The install path routes end-to-end through all seven stages with a per-stage trace in the audit entry. No stage is skipped.
4. `agent-claude` contributes ≤ 800 KiB to the default-build measured on macOS aarch64 and ≤ 1.5 MiB to the `agent-all` build.
5. The total default-build size stays under 15 MiB with the adapter compiled in.
6. The integration test suite is green on macOS aarch64 and Linux x86_64.
