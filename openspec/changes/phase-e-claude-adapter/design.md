# Design: Claude Code adapter + 7-stage install pipeline

> **Status:** ACTIVE · **Date:** 2026-05-18
> **Pairs with:** [proposal.md](proposal.md), [spec.md](spec.md), [tasks.md](tasks.md)
> **Upstream trait surface:** `kernex-adapter-core 0.8.0+` (no changes; this change consumes only)

The ADRs below settle every design decision flagged in [proposal.md §"Ratified design locks"](proposal.md). Each ADR has a context, the locked decision, and the rejected alternatives with the reason they were rejected. ADRs in this document are tested as `E-CC-*` or stage-specific `E-<stage>-*` scenarios in [spec.md](spec.md).

## ADR-001: Pipeline shape and stage signatures

**Context.** The 7-stage configurator pipeline is the spine of this change and a template for future adapter changes. Two design axes: (a) static stage sequence vs. registry-driven, and (b) typed stage outputs vs. untyped JSON blobs.

**Decision.** Static sequence, typed outputs.

`src/configurator/mod.rs` declares:

```rust
pub async fn run(opts: InstallOptions) -> Result<InstallReport, InstallError> {
    let detection = stage_detect::run(&opts).await?;
    let plan = stage_resolve::run(&opts, &detection).await?;
    stage_review::run(&opts, &plan).await?;
    let backup = stage_backup::run(&opts, &plan).await?;
    let apply = stage_apply::run(&opts, &plan, &backup).await
        .or_else(|e| stage_apply::rollback(&backup, e))?;
    let verify = stage_verify::run(&opts, &plan, &apply).await?;
    stage_report::run(&opts, &plan, &apply, &verify).await
}
```

Each stage owns its input and output types in its own module. Stage modules are siblings under `src/configurator/`. No trait abstracts the stage interface in v2.0.

**Rejected alternatives.** (a) Stage trait + registry: deferred to a later change when additional adapters land and stage variance is observable. Premature generalization here would bake assumptions a future change overturns. (b) Untyped JSON between stages: makes the audit log easier to serialize but loses Rust's check on stage-to-stage compatibility. Audit log derives from the typed outputs via `serde`; not the other way around.

## ADR-002: Templates compiled-in via `include_str!`

**Context.** Templates (`CLAUDE.md.tmpl`, `mcp.json.tmpl`, `output-style.md.tmpl`) need to ship inside the binary so air-gapped installs work (E-CC-7). Two options: compile-in via `include_str!` or runtime-load from a templates directory.

**Decision.** Compile-in via `include_str!` in `src/adapters/claude.rs`.

```rust
const CLAUDE_MD_TMPL: &str = include_str!("../../templates/claude/CLAUDE.md.tmpl");
const MCP_JSON_TMPL: &str = include_str!("../../templates/claude/mcp.json.tmpl");
const OUTPUT_STYLE_TMPL: &str = include_str!("../../templates/claude/output-style.md.tmpl");
```

Template substitution uses a hand-rolled `{{key}}` replacer (no template engine dependency; ~30 lines). Keys are restricted to `{{project_name}}`, `{{user_name}}`, `{{kernex_version}}`, `{{install_timestamp}}`, `{{components}}` for v2.0.

**Rejected alternatives.** (a) Runtime template directory at `~/.kx/templates/`: requires bootstrapping the templates somehow (network download, manual copy, or a separate install step), breaks air-gapped install, adds a new failure mode. (b) `tera` / `handlebars`: adds 200+ KiB to the binary against an 800 KiB budget; the substitution surface is too small to justify. The hand-rolled replacer is ~30 lines.

## ADR-003: Audit log shape, format, and append-only semantics

**Context.** The audit log at `~/.kx/audit/install-<ISO8601>.jsonl` is the primary observability surface for install. It is the per-stage trace required by the change's exit signal. It must support: machine-readable parsing, append-only write, rollback records that do not erase prior failure records.

**Decision.** JSONL append-only. One JSON object per line. Filename uses ISO-8601 timestamp with seconds precision so concurrent installs on the same machine never collide.

Event schema:

```json
{
  "event": "stage.detect.start" | "stage.detect.end" | "stage.detect.error" | ...,
  "stage": "detect" | "resolve" | "review" | "backup" | "apply" | "verify" | "report" | "rollback",
  "status": "success" | "failure" | "skipped",
  "started_at": "2026-05-14T10:23:45.123Z",
  "ended_at": "2026-05-14T10:23:45.456Z",
  "duration_ms": 333,
  "payload": { ... stage-specific typed output, redacted of secrets ... },
  "errors": [ { "code": "...", "message": "...", "transient": true|false } ]
}
```

Plus a single `event: "install.summary"` line at the end of every install (success or failure) summarizing the run.

Rollback is recorded as a sequence of `event: "stage.<original>.rollback"` entries; the original failure entries are not modified.

**Rejected alternatives.** (a) Single JSON document written at the end: loses the "what happened before the crash" property; if the process dies mid-install, the file is empty. (b) Plain text log: harder to parse, harder to assert in tests, fails the "structured trace" requirement of the exit signal. (c) Separate trace and audit files: doubles the file count, splits a single causal chain across two artifacts.

## ADR-004: BACKUP stage tarball location and format

**Context.** Stage 4 BACKUP snapshots every file the APPLY stage will touch. Two design choices: (a) per-file copies in a flat directory or (b) a single tarball.

**Decision.** Single tarball at `~/.kx/backups/<ISO8601>-<agent>.tar.gz`. The filename ISO timestamp matches the audit log's ISO timestamp so a backup is trivially correlated with its install. The tarball preserves file permissions and mtime for clean restore.

Restore (used only on rollback or manual revert) is `tar -xzf <backup>.tar.gz -C /`. The audit log records the absolute paths the tarball contains in `payload.files: [...]` so a future `kx audit rollback <install-id>` can verify the backup's coverage before extracting.

**Rejected alternatives.** (a) Per-file copies under `~/.kx/backups/<install-id>/<original-path>.bak`: pollutes the filesystem, makes "what backup belongs to which install" non-obvious. (b) Git-tracked backup directory: introduces a dependency on `git` being installed, breaks on minimal CI runners.

## ADR-005: Automatic rollback on APPLY failure

**Context.** Stage 5 APPLY is where the actual filesystem mutations land. Per-step receipts let us walk backwards on failure. Two design choices for rollback: prompt the user (safe but interactive) or automatic (less safe but matches the audit/structured-trace ethos).

**Decision.** Automatic. On any APPLY step error:

1. APPLY emits `stage.apply.error` to the audit log with the failing receipt.
2. The orchestrator calls `stage_apply::rollback(&backup, e)`.
3. Rollback extracts the backup tarball over the original paths.
4. Each restored file emits `stage.apply.rollback` to the audit log.
5. Top-level `install.summary` records `status: "rolled_back"` and the original error.

The rollback path is itself sandbox-probed (E-CC-2). If the sandbox refuses a restore write, the rollback emits `stage.apply.rollback.error` and the install ends in a "partially rolled back" state. This is the worst-case state this change supports; the audit log fully describes what was and was not restored.

**Rejected alternatives.** (a) Interactive prompt on failure: violates the `--yes` non-interactive contract; complicates CI usage. (b) Manual rollback only (`kx install undo <id>`): leaves the filesystem in a broken state by default, surprises users. (c) "Best-effort" cleanup that swallows rollback errors: hides real failures and makes debugging impossible.

## ADR-006: Internal `Preset` type in kernex-agent; no external preset crate dep

**Context.** The pre-execution audit found that there is no published preset-management crate on crates.io that fits this change's needs. kernex-agent could not depend on a registry crate without first promoting one in a paired upstream release.

Two paths were evaluated:

- **Option A.** Open a paired upstream change to add and publish a preset-management crate, then have kernex-agent depend on it from crates.io. ADR cost: a cross-repo release for an empty crate before any install code lands.
- **Option B.** kernex-agent defines its own internal `Preset` type. No dependency on any external preset crate. A future change later decides whether preset management migrates to a published crate or stays inside the agent.

**Decision.** Option B.

`src/install/preset.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub adapters: Vec<AdapterId>,
    pub components: Vec<String>,
}

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

`AdapterId` is imported from `kernex-adapter-core 0.8.0+` (already a kernex-agent dep after §1 wiring; see ADR-001). The internal `Preset` type is intentionally non-public outside the crate so a future change has full freedom to refactor it (rename, move to a published crate, extend with new fields) without breaking a stable API surface.

**Rejected alternatives.** (a) Option A — paired upstream release for an empty crate adds a release cycle for zero behavior. (b) `git` dep on an upstream crate from kernex-agent — couples build to repo URL stability and breaks `cargo install kernex-agent` for end users. (c) Vendor the `Preset` type via `include!` of a file copied from upstream — duplication without the benefits of a real shared crate. (d) Hardcode the preset in `cli.rs` — spreads preset logic across two modules; an `src/install/preset.rs` keeps the seam a future change needs.

## ADR-007: Per-step receipts inside APPLY

**Context.** APPLY runs N component writes (CLAUDE.md, mcp.json, optionally output-style.md). The rollback walk needs to know which writes completed and which did not.

**Decision.** Each component write returns a `Receipt`:

```rust
pub struct Receipt {
    pub component: String,
    pub path: PathBuf,
    pub action: ReceiptAction, // Created | Overwrote | Skipped
    pub bytes_written: u64,
    pub sha256: [u8; 32],
}
```

APPLY accumulates receipts in a `Vec<Receipt>` and emits them to the audit log on success. On failure, the orchestrator iterates the accumulated receipts in reverse order, restoring each path from the backup tarball.

**Rejected alternatives.** (a) Side-effect-free dry run before writes: doubles APPLY's runtime against a stage that should be fast; the BACKUP stage already gives us the dry-run-equivalent property (we know exactly which paths will be touched). (b) Receipts without checksums: makes the audit log less useful for after-the-fact verification ("did this file get mutated post-install"). The sha256 is 32 bytes and trivial to compute.

## ADR-008: VERIFY stage scope

**Context.** Stage 6 VERIFY runs health checks after APPLY. Aggressive verification (spawn `claude` and ask it to read a CLAUDE.md fact) gives strong signal but couples the install to Claude Code being installed and runnable in the CI environment. Lenient verification (check files exist and parse) is fast and CI-friendly but weaker signal.

**Decision.** Lenient by default; aggressive opt-in via `--verify-deep`.

Default VERIFY runs:

1. Every receipt path exists and matches its receipt sha256.
2. `mcp.json` parses as valid JSON and contains the expected server entry.
3. `CLAUDE.md` parses as Markdown (trivial: any bytes parse as Markdown; this checks UTF-8 validity).

`--verify-deep` additionally runs (only if `claude` is on `$PATH`):

4. `claude --version` exits 0.
5. A canary prompt that reads a known sentinel from CLAUDE.md returns the sentinel (a future change owns the canary template; this change leaves the deep-verify path stubbed with a `tracing::warn!`).

**Rejected alternatives.** (a) Always deep-verify: blocks the exit signal on CI runners that lack `claude` on `$PATH`; couples this change to the canary infrastructure. (b) No verify stage: makes APPLY's "it wrote, ship it" claim untrustworthy; the exit signal explicitly requires a verify stage in the 7-stage list.

## ADR-009: Size budget enforcement at PR time, not at merge time

**Context.** The per-adapter size discipline mandates `agent-claude` contributes ≤ 800 KiB to the default-build. Two design choices: enforce at PR comment time (informational, soft) or at CI gate time (hard fail).

**Decision.** Hard fail in CI. The existing `binary-size-build.yml` workflow gets a new step that computes `delta_agent_claude = default_build_size_with_agent_claude - default_build_size_without_agent_claude` and fails the workflow if `delta_agent_claude > 819200` bytes (800 KiB).

Per-adapter delta measurement is done by building twice in the workflow: once with `--features memory-cli` (no Claude adapter) and once with the full default set. The two numbers exist on every PR already; the delta is a subtraction.

**Rejected alternatives.** (a) Soft warning only: the FU-A-06 incident (15.05 MiB direct-to-main breach) showed soft warnings get ignored; per-adapter discipline needs the same teeth as the variant ceiling. (b) Measure only with the adapter compiled in: cannot separate the adapter's contribution from baseline drift; defeats the per-adapter rule.

## ADR-010: Sandbox refusal probe before write

**Context.** E-CC-2 requires every file write to be sandbox-probed before the write. `kernex-sandbox` (Seatbelt on macOS, Landlock on Linux) supports a probe API for "would this write succeed if attempted now."

**Decision.** Every component write in APPLY calls `kernex_sandbox::probe_write(&path)?` before the actual `fs::write`. The probe is a no-op on platforms where the sandbox is not active (CI runners without the kernel feature). On a refusal, the write returns `InstallError::Transient(SandboxRefused { path })` which maps to `CliError` exit 7 per the broadened classifier from FU-D-AG-05.

The probe is not a guarantee against a TOCTOU race; the actual write may still fail. The point of the probe is to catch the common case (target path is outside the sanctioned roots) before the audit log records a half-completed write.

**Rejected alternatives.** (a) Skip the probe and let the actual write fail: pushes the failure deeper into the stack, complicates the audit trail (the write event would record an attempted but refused write rather than a clean skip). (b) Probe at DETECT and cache the result: TOCTOU window is larger; permissions change between DETECT and APPLY in real systems.

## Trait surface diff summary

This change does not add or modify any public trait or type in `kernex-adapter-core`, `kernex-runtime`, or `kernex-memory`. All changes are confined to `kernex-dev/kernex-agent`. The trait surface from `kernex-adapter-core 0.8.0+` is consumed as-is:

- `Adapter` trait with `id()`, `supports()`, `detect()`, `install_command()`.
- `AdapterId::ClaudeCode` is the registered identity.
- `Capability::{Skills, Memory, Mcp, OutputStyle}` are reported per the templates landed.
- `Detection` populates `installed`, `config_root`, `version` from `which claude` and `claude --version` parsing.
- `AdapterError` wraps any I/O or serde failure from the adapter's own probes.

A future-adapter change may surface a trait extension need; this change does not.

## Open questions deliberately deferred

These are NOT resolved in this change. Each has a target follow-up and a rationale.

- **E-OQ-1: Should `kx install` be idempotent?** A second `kx install --agent claude-code` against a HOME that already has Kernex bits should detect, plan, ask. DETECT stage reports `installed: true` but does not branch behavior on it; APPLY overwrites with backup. **Deferred** to when the configurator generalizes; the idempotency model needs to be uniform across adapters.
- **E-OQ-2: Backup retention.** When does an install delete old backups? **Deferred.** This change writes; nothing prunes. A future `kx audit prune` or background job owns retention.
- **E-OQ-3: Multi-user `$HOME` semantics.** This change targets the invoking user's `$HOME`. System-wide installs (`/etc/skel/`) are not supported. **Deferred to v2.1+.**
- **E-OQ-4: Networked install verification.** Should VERIFY phone home to Anthropic to confirm MCP registration succeeded? **No.** E-CC-7 forbids network calls. Networked verify, if ever, is a separate `kx verify --online` command.
- **E-OQ-5: Audit log rotation.** `~/.kx/audit/install-*.jsonl` accumulates forever. **Deferred.** Same owner as E-OQ-2.
