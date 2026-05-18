//! Stage 6 VERIFY — health checks after APPLY commits.
//!
//! Behavior lands in §10 (E-verify-1..6). VERIFY is observational; failed
//! checks are recorded but do NOT trigger rollback (E-verify-5).

use serde::{Deserialize, Serialize};

use crate::install::audit::AuditWriter;

use super::stage_apply::Receipt;
use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

/// Per-check pass/fail record assembled by VERIFY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub checks: Vec<VerifyCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyCheck {
    pub name: String,
    pub passed: bool,
    pub detail: Option<String>,
}

pub async fn run(
    _opts: &InstallOptions,
    _plan: &InstallPlan,
    _receipts: &[Receipt],
    _audit: &AuditWriter,
) -> Result<VerifyReport, InstallError> {
    unimplemented!("stage_verify::run — lands in §10 of the SDD")
}
