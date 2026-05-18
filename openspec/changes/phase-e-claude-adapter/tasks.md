# Tasks: Claude Code adapter + 7-stage install pipeline

> **Status:** ACTIVE · **Date:** 2026-05-18
> **Pairs with:** [proposal.md](proposal.md), [design.md](design.md), [spec.md](spec.md)
> **Estimated:** ~12-14 atomic commits.

Each section maps to ADRs in [design.md](design.md) and scenarios in [spec.md](spec.md). Implementation order matters: §0 → §1 → §2 → §3 → §4 → §5 → ... → §13. Earlier sections compile before later ones.

## §0 Pre-execution audit

- [x] **0.1** Verify `kernex-adapter-core` and `kernex-sandbox` at the workspace pin (≥ 0.8.0) are live on crates.io via `cargo search`. **2026-05-18: confirmed at 0.8.1 (both crates).**
- [x] **0.2** Verify `src/adapters/claude.rs` is still the one-line reserved file (`//! Reserved for the Claude adapter implementation.`). **2026-05-18: confirmed (line 1, EOF).**
- [x] **0.3** Verify no `src/install/`, `src/configurator/`, or `src/install_pipeline/` directories exist in kernex-agent. **2026-05-18: confirmed all three absent.**
- [x] **0.4** Re-run the `catch_unwind` / `#[should_panic]` / `panic::set_hook` / `panic::take_hook` / `resume_unwind` audit across kernex-agent at the current HEAD. **2026-05-18: 0 hits across `src/` and `tests/`.**
- [x] **0.5** Capture the pre-change default-build baseline. **2026-05-18 at `kernex-dev/kernex-agent@49e2218`, kernex-* 0.8.0:** `target/release/kx` = **11,113,824 bytes (10.60 MiB)** on macOS aarch64. The `size_with` for this change must stay ≤ `11,113,824 + 819,200 = 11,933,024 bytes (11.38 MiB)` to satisfy E-size-1 / E-LOCK-08. The default-build at that point sits 3.62 MiB below the 15 MiB ceiling.

## §1 Configurator scaffolding

- [ ] **1.1** Create `src/configurator/mod.rs` with the `InstallOptions`, `InstallReport`, `InstallError` types per ADR-001.
- [ ] **1.2** Create seven sibling modules: `stage_detect.rs`, `stage_resolve.rs`, `stage_review.rs`, `stage_backup.rs`, `stage_apply.rs`, `stage_verify.rs`, `stage_report.rs`. Each declares its `Input` / `Output` types and a stub `pub async fn run(...) -> Result<Output, InstallError> { unimplemented!() }`.
- [ ] **1.3** Wire `configurator::run(opts)` to call the seven stages in order per the ADR-001 snippet. Behind `#[cfg(feature = "agent-claude")]` for now; gating relaxes when a future change adds peer adapters.
- [ ] **1.4** Add `pub mod configurator;` to `src/lib.rs`. Verify `cargo check` passes.
- [ ] **1.5** Three smoke unit tests:
  - `configurator_compiles_with_all_stages_unimplemented` (this is the "is the module wired" test).
  - `install_options_round_trip_serde` (asserts the `InstallOptions` struct is `Serialize + Deserialize`).
  - `install_error_implements_thiserror` (asserts the `InstallError` enum has the expected variants and renders cleanly).

## §2 Audit writer

- [ ] **2.1** Create `src/install/audit.rs` with `AuditWriter { path: PathBuf, file: Mutex<BufWriter<File>> }` and methods `new(home: &Path) -> Result<Self, AuditError>`, `emit(&self, event: AuditEvent) -> Result<(), AuditError>`.
- [ ] **2.2** Define `AuditEvent` as a `#[derive(Serialize)]` struct matching the JSON schema in ADR-003. Use `chrono::Utc::now()` for timestamps; serialize as RFC 3339 with millisecond precision.
- [ ] **2.3** Implement the filename collision suffix per E-audit-2: if `install-<ts>.jsonl` exists, try `install-<ts>-1.jsonl`, `install-<ts>-2.jsonl`, etc., up to 1000 before erroring.
- [ ] **2.4** Implement the secret redactor per E-audit-6 as a `redact_payload(serde_json::Value) -> serde_json::Value` helper. Tested standalone.
- [ ] **2.5** Five unit tests under `tests/install_audit.rs`:
  - `emits_one_line_per_event_with_trailing_newline` (E-audit-3).
  - `flushes_after_each_event` (E-audit-4; mock writer).
  - `creates_audit_dir_if_missing` (E-audit-1; tempdir).
  - `collision_suffix_resolves_when_same_second_install` (E-audit-2).
  - `redacts_secret_keys_in_payload` (E-audit-6).

## §3 Preset (internal type)

- [ ] **3.1** Create `src/install/preset.rs` defining the internal `Preset` struct (`adapters: Vec<AdapterId>`, `components: Vec<String>`) plus `resolve_preset(name: &str) -> Result<Preset, InstallError>` per [design.md ADR-006](design.md). `AdapterId` is imported from `kernex-adapter-core 0.8.0+`. The `Preset` type is crate-internal (no `pub` outside the install module).
- [ ] **3.2** Implementation: hardcoded `match` on the preset name. For this change only `solo-dev` is wired:
  ```rust
  pub fn resolve_preset(name: &str) -> Result<Preset, InstallError> {
      match name {
          "solo-dev" => Ok(Preset {
              adapters: vec![AdapterId::ClaudeCode],
              components: vec!["claude-md".into(), "mcp-json".into(), "output-style".into()],
          }),
          other => Err(InstallError::UnknownPreset(other.to_string())),
      }
  }
  ```
- [ ] **3.3** No dependency on an external preset crate. A future change decides whether preset management migrates to a published crate (and at that point this module is refactored or removed).
- [ ] **3.4** Three unit tests:
  - `solo_dev_returns_expected_preset`.
  - `unknown_preset_errors_with_unknown_preset_variant`.
  - `preset_is_clone_and_serialize` (forward-compat test ensuring the struct stays trivially serializable for the install plan in the audit log).

## §4 Claude adapter implementation

- [ ] **4.1** Create `src/adapters/claude.rs` implementing `kernex_adapter_core::Adapter` for a unit struct `ClaudeAdapter`.
- [ ] **4.2** Implement `id()`, `supports()`, `detect()`, `install_command()` per E-claude-1..4. `detect()` uses `which::which("claude")` (or shells out via `Command::new("which")`) and `Command::new("claude").arg("--version").output()`.
- [ ] **4.3** Define the three template constants via `include_str!` per ADR-002.
- [ ] **4.4** Implement the template substituter as `pub fn render(tmpl: &str, vars: &HashMap<&str, String>) -> String`. Replaces `{{key}}` with the value; leaves unknown keys literal and emits a `tracing::warn!`.
- [ ] **4.5** Add `pub mod claude;` to `src/adapters/mod.rs` behind `#[cfg(feature = "agent-claude")]`.
- [ ] **4.6** Register `ClaudeAdapter` in a new `pub fn default_registry() -> AdapterRegistry` in `src/adapters/mod.rs`. The configurator calls this at startup.
- [ ] **4.7** Six unit tests under `tests/adapter_claude.rs`:
  - `id_returns_claude_code`.
  - `supports_reports_skills_memory_mcp_output_style`.
  - `detect_returns_installed_false_when_no_claude_on_path` (mock `PATH=/nonexistent`).
  - `render_substitutes_known_keys`.
  - `render_leaves_unknown_keys_literal`.
  - `mcp_json_template_renders_to_valid_json` (E-claude-7).

## §5 Stage 1 DETECT

- [ ] **5.1** Implement `stage_detect::run(opts: &InstallOptions) -> Result<Detection, InstallError>`.
- [ ] **5.2** Internally: look up the adapter from the registry by `opts.agent`. Call `adapter.detect().await`. Emit `stage.detect.start` and `stage.detect.end` to the audit writer.
- [ ] **5.3** Four unit tests:
  - `emits_start_and_end_events` (E-detect-1).
  - `returns_installed_false_when_claude_missing` (E-detect-4).
  - `does_not_write_files_outside_audit_log` (E-detect-3; tempdir).
  - `no_network_call` (E-CC-7 / E-detect-5; use a sentinel resolver).

## §6 Stage 2 RESOLVE

- [ ] **6.1** Implement `stage_resolve::run(opts: &InstallOptions, detection: &Detection) -> Result<InstallPlan, InstallError>`.
- [ ] **6.2** Build the `InstallPlan` from opts.preset (via `resolve_preset`) and opts.agent (via the registry).
- [ ] **6.3** Validate component/agent compatibility (this change: all components in `solo-dev` are Claude-compatible; the check is a stub).
- [ ] **6.4** Emit `stage.resolve.start` and `stage.resolve.end` with the plan in payload.
- [ ] **6.5** Five unit tests covering E-resolve-1..5.

## §7 Stage 3 REVIEW

- [ ] **7.1** Implement `stage_review::run(opts: &InstallOptions, plan: &InstallPlan) -> Result<(), InstallError>`.
- [ ] **7.2** Print the plan to stdout in human-readable form.
- [ ] **7.3** Branch on `opts.yes` and `opts.dry_run` per E-review-1..5.
- [ ] **7.4** TTY detection via `atty::is(atty::Stream::Stdout)`. Non-TTY without `--yes` aborts.
- [ ] **7.5** Five unit tests; the TTY branch uses a `PipedStdin` mock.

## §8 Stage 4 BACKUP

- [ ] **8.1** Implement `stage_backup::run(opts: &InstallOptions, plan: &InstallPlan) -> Result<BackupReceipt, InstallError>`.
- [ ] **8.2** Compute the backup tarball path: `${HOME}/.kx/backups/<UTC-ISO8601>-<agent>.tar.gz`.
- [ ] **8.3** Use `tar::Builder` over a `flate2::write::GzEncoder` over `File::create(path)`.
- [ ] **8.4** Iterate `plan.target_paths`; for each path that exists, add it to the archive preserving permissions and mtime.
- [ ] **8.5** Probe-then-write per E-CC-2.
- [ ] **8.6** Emit `stage.backup.end` with `payload.tarball_path`, `payload.files`, `payload.bytes`.
- [ ] **8.7** Six unit tests covering E-backup-1..6.

## §9 Stage 5 APPLY

- [ ] **9.1** Implement `stage_apply::run(opts: &InstallOptions, plan: &InstallPlan, backup: &BackupReceipt) -> Result<Vec<Receipt>, InstallError>`.
- [ ] **9.2** Iterate `plan.target_paths` in declaration order. For each:
  - Render the corresponding template via `ClaudeAdapter::render`.
  - Compute sha256 of the rendered output.
  - Probe-write per E-CC-2.
  - Write the file (`tokio::fs::write`).
  - Emit `stage.apply.write` with the Receipt.
  - Push the Receipt to the accumulator.
- [ ] **9.3** Implement `stage_apply::rollback(backup: &BackupReceipt, receipts: &[Receipt], err: &InstallError) -> Result<(), InstallError>`:
  - Walk receipts in reverse.
  - For `action: Created`, `fs::remove_file(path)`.
  - For `action: Overwrote`, extract the corresponding entry from the backup tarball to the original path.
  - Emit `stage.apply.rollback` or `stage.apply.rollback.error` per E-apply-5.
- [ ] **9.4** Defensive assertion: assert every write target is in `plan.target_paths`; if not, return `InstallError::PathNotInPlan` (E-apply-8).
- [ ] **9.5** Eight unit tests covering E-apply-1..8 plus E-CC-2 / E-CC-7.

## §10 Stage 6 VERIFY

- [ ] **10.1** Implement `stage_verify::run(opts: &InstallOptions, plan: &InstallPlan, receipts: &[Receipt]) -> Result<VerifyReport, InstallError>`.
- [ ] **10.2** For each Receipt: read the path, verify it exists, compute sha256, compare to receipt.sha256.
- [ ] **10.3** Parse `mcp.json` via `serde_json::from_str::<serde_json::Value>` and assert the `mcpServers` key exists.
- [ ] **10.4** Parse `CLAUDE.md` as UTF-8 (just `std::str::from_utf8`).
- [ ] **10.5** Behind `--verify-deep`: if `which("claude").is_ok()`, run `claude --version` and emit `stage.verify.deep_version`. Stub the canary prompt with a `tracing::warn!`.
- [ ] **10.6** Failed checks do NOT trigger rollback (E-verify-5). They are recorded in the VerifyReport.
- [ ] **10.7** Six unit tests covering E-verify-1..6.

## §11 Stage 7 REPORT

- [ ] **11.1** Implement `stage_report::run(opts, plan, apply, verify) -> Result<InstallReport, InstallError>`.
- [ ] **11.2** Print the human-readable summary per E-report-1.
- [ ] **11.3** Compose the `install.summary` event per E-report-2 and emit to the audit writer.
- [ ] **11.4** Map outcomes to exit codes per E-report-3 by returning a typed `InstallReport` the CLI dispatcher converts.
- [ ] **11.5** Three unit tests covering the exit-code mapping for `success`, `rolled_back`, `user_declined`.

## §12 CLI wiring

- [ ] **12.1** Add the `install` subcommand to the `kx` clap definition. Flags: `--agent <name>`, `--preset <name>`, `--yes`, `--dry-run`, `--verify-deep`.
- [ ] **12.2** Dispatch handler: parse flags into `InstallOptions`, call `configurator::run(opts).await`, map the typed report to an exit code.
- [ ] **12.3** Wire the audit writer into `InstallOptions` via a `home: PathBuf` (default `$HOME`, override `$KX_HOME` for tests).
- [ ] **12.4** Update `src/main.rs` and the help-text contract (CC-8 from the memory CLI SDD) for the new subcommand.
- [ ] **12.5** Three integration tests under `tests/install_claude_happy_path.rs`, `tests/install_claude_rollback.rs`, `tests/install_claude_dry_run.rs`:
  - Happy path: tempdir HOME, `kx install --agent claude-code --preset solo-dev`, assert all four exit-signal clauses (files present, audit log shape, per-stage trace, total size).
  - Rollback path: inject a sandbox refusal at the second component, assert rollback restored the prior state and the audit log records the failure + rollback events.
  - Dry-run path: assert no files are written, all three pre-APPLY stages emit events, exit 0.

## §13 Size measurement

- [ ] **13.1** Update `kernex-dev/kernex-agent/.github/workflows/binary-size-build.yml` to add a `delta_agent_claude` measurement step.
- [ ] **13.2** Two builds in the same job: `cargo build --release --features memory-cli` (size_without) and `cargo build --release` (size_with).
- [ ] **13.3** Compute `delta = size_with - size_without`. Fail the workflow with a clear error message if `delta > 819200` bytes (800 KiB).
- [ ] **13.4** Update the existing binary-size PR comment to include the delta line.
- [ ] **13.5** Capture the post-change measurement at the implementation PR's HEAD; record in the post-merge note.

## Definition of done

This change ships when:

1. `kx install --agent claude-code --preset solo-dev` against a clean `$HOME` tempdir produces a valid `CLAUDE.md`, `mcp.json`, `output-style.md`.
2. `~/.kx/audit/install-<ISO8601>.jsonl` exists with one event per stage plus an `install.summary` event.
3. Every spec scenario in [spec.md](spec.md) has a corresponding green test (unit or integration). The test names match the scenario IDs.
4. `delta_agent_claude ≤ 800 KiB` measured on macOS aarch64 release builds.
5. The default-build size stays under 11.5 MiB.
6. CI is green on macOS aarch64 and Linux x86_64.

If any of (1)–(5) fail, the implementation PR does not merge; the SDD revises and the cycle restarts.
