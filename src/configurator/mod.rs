//! 7-stage install configurator pipeline.
//!
//! See `openspec/changes/phase-e-claude-adapter/design.md` ADR-001 for the
//! pipeline shape. Each stage owns its input and output types in its own
//! module under `src/configurator/`. The orchestrator below threads typed
//! values between stages and short-circuits to rollback on APPLY failure.
//!
//! This module ships scaffold-only in §1: every stage `run()` returns
//! `unimplemented!()` and the orchestrator is wired but inert. Behavior
//! lands in §5-§11 (one stage per commit).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::install::audit::AuditWriter;

pub mod stage_apply;
pub mod stage_backup;
pub mod stage_detect;
pub mod stage_report;
pub mod stage_resolve;
pub mod stage_review;
pub mod stage_verify;

/// User-supplied options parsed from the `kx install` CLI surface.
///
/// Mirrors the clap flags in §12.1; serializable so the audit log can
/// record the inputs verbatim under `install.summary.payload.options`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallOptions {
    /// `--agent <name>`. Resolved to an `AdapterId` at RESOLVE.
    pub agent: String,
    /// `--preset <name>`. Resolved via `resolve_preset` at RESOLVE (§3).
    pub preset: String,
    /// `--yes` skips the REVIEW prompt.
    pub yes: bool,
    /// `--dry-run` exits cleanly after REVIEW without invoking BACKUP onward.
    pub dry_run: bool,
    /// `--verify-deep` adds the deep-verify stub on top of the lenient default.
    pub verify_deep: bool,
    /// Invoking user's `$HOME`. Tests override via `$KX_HOME` (see §12.3).
    pub home: PathBuf,
    /// Project-local working directory at `kx install` invocation time.
    /// `None` lets the configurator fall back to `std::env::current_dir()`,
    /// which is what the production CLI dispatcher sets when launching
    /// from a user shell. Tests pass an explicit `TempDir` so the
    /// project-local Codex `<cwd>/AGENTS.md` write lands inside the
    /// per-test fixture instead of the runner's working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// Final typed report from the REPORT stage. The CLI dispatcher converts
/// this into an exit code per E-report-3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReport {
    pub status: InstallStatus,
    pub audit_log_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub components_written: Vec<String>,
}

/// Terminal status of an install run. Drives the exit code mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Success,
    SuccessWithVerifyFailures,
    UserDeclined,
    RolledBack,
    Aborted,
}

/// Install pipeline error surface.
///
/// The classifier shape matches the memory CLI's `CliError::Transient`
/// pattern (FU-D-AG-05): transient variants map to exit 7, hard variants
/// to exit 5, usage variants to exit 2. The wiring lands in §11.4 and §12.2.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("unknown agent '{0}'")]
    UnknownAgent(String),
    #[error("unknown preset '{0}'")]
    UnknownPreset(String),
    #[error("path '{0}' is not in the resolved install plan")]
    PathNotInPlan(PathBuf),
    #[error("sandbox refused write at '{path}'")]
    SandboxRefused { path: PathBuf },
    #[error("permanent install failure: {0}")]
    Permanent(String),
    #[error("transient install failure: {0}")]
    Transient(String),
    #[error("user declined at REVIEW")]
    UserDeclined,
    #[error("non-interactive context without --yes")]
    NonInteractive,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Top-level pipeline entry point per ADR-001.
///
/// Each stage takes the prior stage's typed output and returns its own.
/// APPLY failure short-circuits to rollback; the caller observes a typed
/// `InstallReport` either way. The `AuditWriter` lives for the duration
/// of a single install run; one fresh log file under `~/.kx/audit/`.
pub async fn run(opts: InstallOptions) -> Result<InstallReport, InstallError> {
    let audit = AuditWriter::new(&opts.home)
        .map_err(|e| InstallError::Permanent(format!("open audit log: {e}")))?;
    run_with_audit(opts, &audit).await
}

/// Same as `run` but with an injected audit writer (tests, advanced callers).
pub async fn run_with_audit(
    opts: InstallOptions,
    audit: &AuditWriter,
) -> Result<InstallReport, InstallError> {
    let detection = stage_detect::run(&opts, audit).await?;
    let plan = stage_resolve::run(&opts, &detection, audit).await?;
    stage_review::run(&opts, &plan, audit).await?;
    if opts.dry_run {
        return stage_report::run_dry_run(&opts, &plan, audit).await;
    }
    let backup = stage_backup::run(&opts, &plan, audit).await?;
    let apply = match stage_apply::run(&opts, &plan, &backup, audit).await {
        Ok(receipts) => receipts,
        Err(stage_apply::ApplyFailure { partial, error }) => {
            // Roll back the components written before the failure. Passing the
            // partial receipts (not an empty slice) is what makes auto-rollback
            // actually undo a partial install.
            let _ = stage_apply::rollback(&backup, &partial, &error, audit).await;
            return Err(error);
        }
    };
    let verify = stage_verify::run(&opts, &plan, &apply, audit).await?;
    stage_report::run(&opts, &plan, &apply, &verify, audit).await
}
