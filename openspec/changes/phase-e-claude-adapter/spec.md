# Spec: Claude Code adapter + 7-stage install pipeline

> **Status:** ACTIVE · **Date:** 2026-05-18
> **Pairs with:** [proposal.md](proposal.md), [design.md](design.md), [tasks.md](tasks.md)
> **Test naming convention:** every scenario has an ID (`E-CC-N`, `E-detect-N`, etc). Tests reference these IDs in their names, e.g. `test_e_cc_1_audit_log_is_only_write_surface`.

## Cross-cutting invariants

These hold across every stage and are tested as standalone scenarios.

- **E-CC-1.** Every file write outside test fixtures goes through `src/install/audit.rs`. A negative test ensures no other module calls `fs::write` or `tokio::fs::write` directly (enforced by a `grep` check in CI, not a runtime assertion).
- **E-CC-2.** Every file write is preceded by a `kernex_sandbox::probe_write(&path)` call. A sandbox refusal returns `InstallError::Transient(SandboxRefused { path })` mapping to `CliError` exit 7.
- **E-CC-3.** Every stage emits exactly one `stage.<name>.start` and one `stage.<name>.end` event (or `stage.<name>.error`) to the audit log. No stage runs without bracketing trace events.
- **E-CC-4.** Failures roll back automatically. The audit log records the failure event, every restoration event, and an `install.summary` with `status: "rolled_back"`.
- **E-CC-5.** The default-build size delta for `agent-claude` is measured in CI and is ≤ 800 KiB. A delta > 800 KiB fails the workflow per ADR-009.
- **E-CC-6.** No write target is outside `$HOME`. A test runs `kx install --agent claude-code --preset solo-dev` against a `TempDir` HOME and asserts no path under `/`, `/etc`, `/usr`, `/var`, `/opt` is touched.
- **E-CC-7.** No network call during install. The integration tests run with `--features memory-cli,agent-claude` and assert via a `reqwest::Client` blocker that no DNS resolution or TCP connect is attempted.

## `kx install` CLI surface

- **E-install-1.** `kx install --agent claude-code --preset solo-dev` returns exit 0 on the happy path.
- **E-install-2.** `kx install --agent claude-code --preset solo-dev --dry-run` runs stages DETECT, RESOLVE, REVIEW, prints the plan to stdout, and exits 0 without invoking BACKUP, APPLY, VERIFY, REPORT.
- **E-install-3.** `kx install --agent claude-code --preset solo-dev --yes` skips the REVIEW prompt; non-yes runs prompt for confirmation on a TTY and abort on a non-TTY.
- **E-install-4.** `kx install --agent unknown-agent --preset solo-dev` fails fast at RESOLVE with exit 2 (usage error) and writes a single `stage.resolve.error` event to the audit log.
- **E-install-5.** `kx install --agent claude-code --preset unknown-preset` fails fast at RESOLVE with exit 2; the inline-default shim only catches known scaffold-empty preset names.
- **E-install-6.** `kx install --agent claude-code --preset solo-dev --verify-deep` runs the default VERIFY plus the deep-verify stub; the stub emits a `tracing::warn!` and does not block the exit code.
- **E-install-7.** Missing `--agent` or `--preset` is a clap error (exit 2) printed before any stage runs.

## Stage 1: DETECT

- **E-detect-1.** DETECT writes one `stage.detect.start` and one `stage.detect.end` event to the audit log.
- **E-detect-2.** DETECT returns a `Detection` with `installed: bool` (true iff `which claude` succeeds), `config_root: Option<PathBuf>` (set to `$HOME/.claude/` if it exists or is creatable), `version: Option<String>` (parsed from `claude --version`).
- **E-detect-3.** DETECT does not write any file outside the audit log. It can read `$HOME/.claude/` and run `which claude` / `claude --version`.
- **E-detect-4.** DETECT does not fail if `claude` is not on `$PATH`. It returns `installed: false` and lets the orchestrator decide.
- **E-detect-5.** DETECT does not make network calls (E-CC-7).
- **E-detect-6.** DETECT completes in under 500 ms on the happy path (informational; failing this is not a spec violation but a perf regression).

## Stage 2: RESOLVE

- **E-resolve-1.** RESOLVE takes the user's flags and the DETECT output, returns an `InstallPlan` with `agent: AdapterId`, `components: Vec<String>`, `target_paths: Vec<(component, path)>`.
- **E-resolve-2.** RESOLVE expands the preset via `src/install/preset.rs`. For `solo-dev` it returns the inline default; for unknown presets it returns `InstallError::UnknownPreset`.
- **E-resolve-3.** RESOLVE refuses incompatible component/agent pairs cleanly (none exist in this change; this scenario covers future-adapter needs and is a stub).
- **E-resolve-4.** RESOLVE writes one `stage.resolve.start` and one `stage.resolve.end` event with the resolved plan in `payload`.
- **E-resolve-5.** RESOLVE outputs no files. The plan exists in memory only until BACKUP serializes it.

## Stage 3: REVIEW

- **E-review-1.** REVIEW prints the plan in a human-readable form to stdout (component list with target paths).
- **E-review-2.** On `--yes`, REVIEW does not prompt and emits `stage.review.end` with `status: "skipped_prompt"`.
- **E-review-3.** On no flag and an interactive TTY, REVIEW prompts `Proceed? [y/N]` and reads from stdin. `y` continues; anything else aborts with exit 0 and an `install.summary` with `status: "user_declined"`.
- **E-review-4.** On no flag and a non-interactive context (no TTY), REVIEW aborts with exit 2 and an explanatory error to stderr.
- **E-review-5.** On `--dry-run`, REVIEW prints the plan and the orchestrator exits before BACKUP. No prompt.

## Stage 4: BACKUP

- **E-backup-1.** BACKUP takes the InstallPlan and writes a tarball at `~/.kx/backups/<ISO8601>-<agent>.tar.gz`.
- **E-backup-2.** The tarball contains every file path in the InstallPlan's `target_paths` that already exists on disk. Paths that do not yet exist are recorded in the audit payload but not included in the tarball.
- **E-backup-3.** The tarball preserves file permissions and mtime.
- **E-backup-4.** BACKUP writes a `stage.backup.end` event with `payload.tarball_path`, `payload.files: [...]`, `payload.bytes`.
- **E-backup-5.** If the backups directory does not exist, BACKUP creates it (probed via the sandbox per E-CC-2).
- **E-backup-6.** If tarball creation fails (disk full, permission denied), BACKUP returns `InstallError::Permanent` and the install aborts before APPLY. No partial backup is left on disk; the partial tarball is unlinked.

## Stage 5: APPLY

- **E-apply-1.** APPLY iterates the InstallPlan's components in declaration order. For each, it expands the corresponding template, calls `kernex_sandbox::probe_write(&path)`, then writes the file.
- **E-apply-2.** Each write produces a `Receipt { component, path, action, bytes_written, sha256 }` per ADR-007. The receipt is appended to an in-memory `Vec<Receipt>` and emitted to the audit log as `stage.apply.write` (one event per component).
- **E-apply-3.** On any component write failure, APPLY emits `stage.apply.error` with the failing component and reason, then invokes `stage_apply::rollback(&backup, &receipts, &error)`.
- **E-apply-4.** Rollback walks the accumulated receipts in reverse order. For each receipt with `action: Created`, the file is removed. For each receipt with `action: Overwrote`, the file is restored from the backup tarball.
- **E-apply-5.** Each rollback action emits `stage.apply.rollback` to the audit log. If a rollback action itself fails, it emits `stage.apply.rollback.error` and continues with the remaining receipts (best-effort rollback per ADR-005).
- **E-apply-6.** After successful APPLY, the audit log records `stage.apply.end` with `payload.receipts: [...]`.
- **E-apply-7.** APPLY does not call any network API (E-CC-7).
- **E-apply-8.** APPLY never writes to a path that was not in the InstallPlan. A defensive assertion in debug builds enforces this; in release builds it is a hard error (`InstallError::PathNotInPlan`).

## Stage 6: VERIFY

- **E-verify-1.** Default VERIFY checks that each receipt's path exists, is readable, and matches its receipt sha256.
- **E-verify-2.** Default VERIFY parses `mcp.json` and asserts it contains the expected `mcpServers` entry with the right name.
- **E-verify-3.** Default VERIFY parses `CLAUDE.md` as UTF-8 (any UTF-8 bytes pass; this catches encoding-corrupted writes).
- **E-verify-4.** VERIFY emits `stage.verify.end` with `payload.checks: [...]` listing each check name and pass/fail.
- **E-verify-5.** A failed check is recorded but does NOT trigger rollback (verify is observational, not transactional). The `install.summary` records `status: "verified_with_failures"` and the failing checks. APPLY has already committed.
- **E-verify-6.** With `--verify-deep`, VERIFY additionally attempts `claude --version` and emits `stage.verify.deep_version` events. This change does not implement the canary prompt step; that lands in a follow-up and stays stubbed here with a `tracing::warn!`.

## Stage 7: REPORT

- **E-report-1.** REPORT prints a human-readable summary to stdout: agent installed, components written, backup path, audit log path, suggested next-step command (`kx mem stats`).
- **E-report-2.** REPORT writes the final `install.summary` event to the audit log with: `status` (one of `success`, `success_with_verify_failures`, `rolled_back`, `user_declined`, `aborted`), `started_at`, `ended_at`, `total_duration_ms`, `agent`, `preset`, `components: [...]`, `backup_path`, `audit_log_path`.
- **E-report-3.** Exit code:
  - `success` and `success_with_verify_failures` exit 0.
  - `user_declined` exits 0 (deliberate user choice, not an error).
  - `rolled_back` exits 1 (an attempt was made and reverted).
  - `aborted` exits 2 (usage error or missing prerequisite).
  - Transient errors (sandbox refusal, filesystem lock) exit 7 per FU-D-AG-05.

## Claude adapter (`AdapterId::ClaudeCode`)

- **E-claude-1.** `ClaudeAdapter::id()` returns `AdapterId::ClaudeCode`.
- **E-claude-2.** `ClaudeAdapter::supports(cap)` returns `true` for `Capability::{Skills, Memory, Mcp, OutputStyle}`.
- **E-claude-3.** `ClaudeAdapter::detect()` returns `Detection { installed, config_root, version }` per E-detect-2.
- **E-claude-4.** `ClaudeAdapter::install_command()` returns the canonical Anthropic install one-liner (finalize at PR time after verifying against current docs).
- **E-claude-5.** Templates load via `include_str!` at compile time. Runtime parsing of `CLAUDE.md.tmpl`, `mcp.json.tmpl`, `output-style.md.tmpl` is exercised by unit tests.
- **E-claude-6.** Template substitution replaces `{{project_name}}`, `{{user_name}}`, `{{kernex_version}}`, `{{install_timestamp}}`, `{{components}}`. Unknown keys are left literal (`{{unknown}}` stays in the output) and emit a `tracing::warn!` at TRACE level.
- **E-claude-7.** `mcp.json.tmpl` produces output that parses as valid JSON via `serde_json::from_str`.
- **E-claude-8.** `CLAUDE.md.tmpl` produces output that survives a Markdown roundtrip (no invariants on rendering; just UTF-8 validity).

## Audit log

- **E-audit-1.** The audit log file path is `${HOME}/.kx/audit/install-<UTC-ISO8601-seconds>.jsonl`. The directory is created if missing (probed per E-CC-2).
- **E-audit-2.** Two installs started in the same second produce distinct filenames (collision suffix `-1`, `-2`, etc., resolved by the writer).
- **E-audit-3.** Every event is one JSON object on one line, terminated by `\n`. No trailing whitespace.
- **E-audit-4.** The writer flushes after each event so a process crash leaves a consistent prefix.
- **E-audit-5.** The audit log is never modified after a line is written. Appends only.
- **E-audit-6.** Secrets and PII are redacted from `payload` before write. The redaction rule: any key matching `(?i)token|secret|password|api_key` in a nested map is replaced with `"<redacted>"`. This change does not write any such key today, but the redactor is wired so future adapters do not regress.

## Size budget

- **E-size-1.** A CI workflow step computes `delta_agent_claude = size_with - size_without` on every PR. `size_with` is the default-build size; `size_without` is the build with `--features memory-cli` only.
- **E-size-2.** `delta_agent_claude > 800 KiB` fails the workflow with a clear message.
- **E-size-3.** The PR comment block surfaces both numbers and the delta. The pre-merge default-build size also stays under 11.5 MiB as a leading indicator.

## What this spec deliberately does NOT cover

- **TUI behavior.** This change exposes `--yes`, `--dry-run`, `--verify-deep` and that is the whole flag surface.
- **Other adapters.** Future-adapter changes will add Codex / OpenCode / Cursor / Cline support. The configurator code is shaped to accept them but no scenarios cover them here.
- **Preset catalog.** Only `solo-dev` is wired (inline default). Other presets return `InstallError::UnknownPreset`.
- **Multi-user installs.** `$HOME` is the invoking user's HOME. System-wide install is out of scope.
- **Networked verify.** `--verify-deep` runs the local stub; the networked canary lands in a future change.
- **Audit log rotation, backup pruning.** Both deferred.
- **`kx audit show` / `kx audit rollback` subcommands.** These exist as design hooks (the audit log shape supports them) but their CLI surface is a future deliverable.
