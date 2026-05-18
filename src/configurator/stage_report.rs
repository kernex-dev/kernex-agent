//! Stage 7 REPORT — emit the human-readable summary and the `install.summary` audit event.
//!
//! Behavior lands in §11 (E-report-1..3). REPORT also owns the dry-run
//! exit path (E-install-2): when `opts.dry_run` is set, the orchestrator
//! skips BACKUP onward and calls `run_dry_run` instead.

use crate::install::audit::AuditWriter;

use super::stage_apply::Receipt;
use super::stage_resolve::InstallPlan;
use super::stage_verify::VerifyReport;
use super::{InstallError, InstallOptions, InstallReport};

pub async fn run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
    _apply: &[Receipt],
    _verify: &VerifyReport,
    _audit: &AuditWriter,
) -> Result<InstallReport, InstallError> {
    unimplemented!("stage_report::run — lands in §11 of the SDD")
}

pub async fn run_dry_run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
    _audit: &AuditWriter,
) -> Result<InstallReport, InstallError> {
    unimplemented!("stage_report::run_dry_run — lands in §11 of the SDD")
}
